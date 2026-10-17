//! Surface factory. A [`Backend::Gpu`] surface is a Metal render target on macOS. A DirectContext
//! is not `Send`, so each thread builds its own on first use.

use skia_safe::{Image, ISize, Surface, surfaces};

use crate::RenderError;

/// The resolved backend. Choosing between `auto`, `gpu` and `cpu` happens at the CLI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Cpu,
    Gpu,
}

#[cfg(target_os = "macos")]
mod metal {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice};
    use skia_safe::gpu::{self, DirectContext, mtl};

    thread_local! {
        static CTX: RefCell<Option<DirectContext>> = RefCell::new(make());
    }

    fn make() -> Option<DirectContext> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        // SAFETY: the raw device and queue pointers are owned by the backend context from here on.
        let backend = unsafe {
            mtl::BackendContext::new(
                Retained::into_raw(device) as mtl::Handle,
                Retained::into_raw(queue) as mtl::Handle,
            )
        };
        gpu::direct_contexts::make_metal(&backend, None)
    }

    pub fn available() -> bool {
        CTX.with(|c| c.borrow().is_some())
    }

    pub fn surface(size: skia_safe::ISize) -> Option<skia_safe::Surface> {
        CTX.with(|c| {
            gpu::surfaces::render_target(
                c.borrow_mut().as_mut()?,
                gpu::Budgeted::Yes,
                &skia_safe::ImageInfo::new_n32_premul(size, None),
                None,
                gpu::SurfaceOrigin::TopLeft,
                None,
                false,
                None,
            )
        })
    }

    pub fn to_raster(image: &skia_safe::Image) -> Option<skia_safe::Image> {
        CTX.with(|c| image.make_raster_image(c.borrow_mut().as_mut()?, None))
    }
}

/// Whether this machine can render on the GPU backend.
pub fn gpu_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        metal::available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub(crate) fn surface(backend: Backend, size: impl Into<ISize>) -> Option<Surface> {
    let size = size.into();
    match backend {
        Backend::Cpu => surfaces::raster_n32_premul(size),
        #[cfg(target_os = "macos")]
        Backend::Gpu => metal::surface(size),
        #[cfg(not(target_os = "macos"))]
        Backend::Gpu => None,
    }
}

/// The final image of a surface, read back to CPU memory so it can be encoded or read.
pub(crate) fn finish(backend: Backend, surface: &mut Surface) -> Result<Image, RenderError> {
    let image = surface.image_snapshot();
    match backend {
        Backend::Cpu => Ok(image),
        #[cfg(target_os = "macos")]
        Backend::Gpu => {
            metal::to_raster(&image).ok_or_else(|| RenderError::Skia("gpu readback".into()))
        }
        #[cfg(not(target_os = "macos"))]
        Backend::Gpu => Err(RenderError::Skia("gpu backend unavailable".into())),
    }
}
