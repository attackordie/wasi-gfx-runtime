mod surface;
pub use surface::{
    add_to_linker as add_surface_to_linker, GfxWindow, Key, KeyEvent, MainThreadSpawner,
    PointerEvent, ResizeEvent, Surface, SurfaceCtx, SurfaceCtxView, SurfaceDesc,
};

#[cfg(feature = "winit")]
pub mod winit;

#[cfg(feature = "surface-webgpu")]
mod surface_webgpu;
#[cfg(feature = "surface-webgpu")]
pub use surface_webgpu::{
    add_to_linker as add_surface_webgpu_to_linker, SurfaceWebgpuCtx, SurfaceWebgpuCtxView,
};

#[cfg(feature = "surface-frame-buffer")]
mod surface_frame_buffer;
#[cfg(feature = "surface-frame-buffer")]
pub use surface_frame_buffer::{
    add_to_linker as add_surface_frame_buffer_to_linker, SurfaceFrameBufferCtx,
    SurfaceFrameBufferCtxView,
};

/// Re-exports for embedders whose own `bindgen!` worlds reference these
/// interfaces (e.g. a guest export taking `borrow<context>`) and must
/// `with`-map the resource types to this crate's, or typed instantiation
/// fails with "resource type mismatch".
pub mod reexports {
    pub use crate::surface::wasi_gfx;
    #[cfg(feature = "surface-frame-buffer")]
    pub use crate::surface_frame_buffer::GfxContext as SurfaceFrameBufferContext;
    #[cfg(feature = "surface-webgpu")]
    pub use crate::surface_webgpu::Context as SurfaceWebgpuContext;
}

/// Add surface, surface-webgpu, surface-frame-buffer to the linker
#[cfg(all(feature = "surface-webgpu", feature = "surface-frame-buffer"))]
pub fn add_all_to_linker<T>(l: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
where
    T: SurfaceCtxView + SurfaceWebgpuCtxView + SurfaceFrameBufferCtxView,
{
    add_surface_to_linker(l)?;
    add_surface_webgpu_to_linker(l)?;
    add_surface_frame_buffer_to_linker(l)?;
    Ok(())
}
