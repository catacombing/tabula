//! Wayland window rendering.

use std::path::Path;
use std::ptr::NonNull;

use glutin::display::{Display, DisplayApiPreference};
use image::{ColorType, ImageReader};
use raw_window_handle::{RawDisplayHandle, WaylandDisplayHandle};
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer, LayerSurface};

use crate::cli::Options;
use crate::geometry::{Position, Size};
use crate::renderer::{Renderer, Texture};
use crate::wayland::ProtocolStates;
use crate::{Error, State, gl};

/// Wayland window.
pub struct Window {
    surface: LayerSurface,
    viewport: WpViewport,
    renderer: Renderer,

    options: Options,

    spb_buffer: Option<WlBuffer>,
    image: Option<Image>,

    size: Size,
    scale: f64,
}

impl Window {
    pub fn new(
        protocol_states: &ProtocolStates,
        connection: &Connection,
        queue: &QueueHandle<State>,
        options: Options,
    ) -> Result<Self, Error> {
        // Get EGL display.
        let display = NonNull::new(connection.backend().display_ptr().cast()).unwrap();
        let wayland_display = WaylandDisplayHandle::new(display);
        let raw_display = RawDisplayHandle::Wayland(wayland_display);
        let egl_display = unsafe { Display::new(raw_display, DisplayApiPreference::Egl)? };

        // Create surface's Wayland global handles.
        let surface = protocol_states.compositor.create_surface(queue);
        if let Some(fractional_scale) = &protocol_states.fractional_scale {
            fractional_scale.fractional_scaling(queue, &surface);
        }
        let viewport = protocol_states.viewporter.viewport(queue, &surface);

        // Create the layer shell window.
        let surface = protocol_states.layer_shell.create_layer_surface(
            queue,
            surface,
            Layer::Bottom,
            Some("wallpaper"),
            None,
        );
        surface.set_anchor(Anchor::LEFT | Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM);
        surface.set_exclusive_zone(-1);
        surface.set_size(0, 0);
        surface.commit();

        // Create OpenGL renderer.
        let wl_surface = surface.wl_surface();
        let renderer = Renderer::new(egl_display, wl_surface.clone());

        // Try to load the background image.
        let image = match &options.image {
            Some(image_path) => Some(UnloadedImage::new(image_path)?.into()),
            None => None,
        };

        // If no image is used and SPB is supported, use it to draw the background.
        let spb_buffer =
            protocol_states.single_pixel_buffer.as_ref().filter(|_| options.image.is_none()).map(
                |spb| {
                    let [r, g, b] = [
                        options.color.r as u32 * (u32::MAX / 255),
                        options.color.g as u32 * (u32::MAX / 255),
                        options.color.b as u32 * (u32::MAX / 255),
                    ];
                    spb.create_u32_rgba_buffer(r, g, b, u32::MAX, queue, ())
                },
            );

        Ok(Self {
            spb_buffer,
            viewport,
            renderer,
            options,
            surface,
            image,
            scale: 1.,
            size: Default::default(),
        })
    }

    /// Redraw the window.
    pub fn draw(&mut self) {
        // Update viewporter logical render size.
        //
        // NOTE: This must be done every time we draw with Sway; it is not
        // persisted when drawing with the same surface multiple times.
        self.viewport.set_destination(self.size.width as i32, self.size.height as i32);

        // Mark entire window as damaged.
        let wl_surface = self.surface.wl_surface();
        wl_surface.damage(0, 0, self.size.width as i32, self.size.height as i32);

        // Render the window content.
        match &self.spb_buffer {
            Some(buffer) => wl_surface.attach(Some(buffer), 0, 0),
            None => {
                let physical_size = self.size * self.scale;
                self.renderer.draw(physical_size, |renderer| {
                    Self::gl_render(renderer, physical_size, &mut self.image, &self.options)
                });
            },
        }

        // Apply surface changes.
        wl_surface.commit();
    }

    /// Perform OpenGL rendering.
    fn gl_render(
        renderer: &Renderer,
        physical_size: Size,
        image: &mut Option<Image>,
        options: &Options,
    ) {
        // Render background color.
        let [r, g, b] = [
            options.color.r as f32 / 255.,
            options.color.g as f32 / 255.,
            options.color.b as f32 / 255.,
        ];
        unsafe { gl::ClearColor(r, g, b, 1.) };
        unsafe { gl::Clear(gl::COLOR_BUFFER_BIT) };

        // Render wallpaper image.

        let image = match image {
            Some(image) => image,
            None => return,
        };

        let physical_size: Size<f32> = physical_size.into();
        let image_size: Size<f32> = image.size().into();
        let focus = options.focus;

        // Fit image to screen dimensions.
        let width_ratio = physical_size.width / image_size.width;
        let height_ratio = physical_size.height / image_size.height;
        let (position, size) = if width_ratio < height_ratio {
            let width = image_size.width * height_ratio;
            let x = (physical_size.width - width) * focus.x;
            (Position::new(x, 0.), Size::new(width, physical_size.height))
        } else {
            let height = image_size.height * width_ratio;
            let y = (physical_size.height - height) * focus.y;
            (Position::new(0., y), Size::new(physical_size.width, height))
        };

        unsafe { renderer.draw_texture_at(image.texture(), position, size) };
    }

    /// Update the window's logical size.
    pub fn set_size(&mut self, compositor: &CompositorState, size: Size) {
        if self.size == size {
            return;
        }

        self.size = size;

        // Update the window's opaque region.
        //
        // This is done here since it can only change on resize, but the commit happens
        // atomically on redraw.
        if let Ok(region) = Region::new(compositor) {
            region.add(0, 0, size.width as i32, size.height as i32);
            self.surface.wl_surface().set_opaque_region(Some(region.wl_region()));
        }

        self.draw();
    }

    /// Update the window's DPI factor.
    pub fn set_scale_factor(&mut self, scale: f64) {
        if self.scale == scale {
            return;
        }

        self.scale = scale;

        if self.size != Size::default() {
            self.draw();
        }
    }
}

/// OpenGL renderable image.
enum Image {
    Loaded(Texture),
    Unloaded(UnloadedImage),
}

impl Image {
    /// Get this image's OpenGL texture.
    ///
    /// # Safety
    ///
    /// This must be called with the correct context made current, or the image
    /// will be loaded into an unrelated context.
    unsafe fn texture(&mut self) -> &Texture {
        // Load the OpenGL texture.
        if let Self::Unloaded(image) = self {
            let texture = Texture::new(&image.bytes, image.width, image.height, image.gl_format);
            *self = Self::Loaded(texture);
        }

        match self {
            Self::Loaded(texture) => texture,
            Self::Unloaded(_) => unreachable!(),
        }
    }

    /// Source image dimensions.
    fn size(&self) -> Size {
        match &self {
            Self::Loaded(texture) => Size::new(texture.width, texture.height),
            Self::Unloaded(image) => Size::new(image.width, image.height),
        }
    }
}

impl From<UnloadedImage> for Image {
    fn from(image: UnloadedImage) -> Self {
        Self::Unloaded(image)
    }
}

/// Raw wallpaper image data.
struct UnloadedImage {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    gl_format: u32,
}

impl UnloadedImage {
    fn new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let image = ImageReader::open(path)?.decode()?;

        let width = image.width();
        let height = image.height();

        let (bytes, gl_format) = match image.color() {
            ColorType::La8 => (image.into_luma_alpha8().into_raw(), gl::LUMINANCE_ALPHA),
            ColorType::L8 => (image.into_luma8().into_raw(), gl::LUMINANCE),
            ColorType::Rgb8 => (image.into_rgb8().into_raw(), gl::RGB),
            _ => (image.into_rgba8().into_raw(), gl::RGBA),
        };

        Ok(Self { gl_format, width, height, bytes })
    }
}
