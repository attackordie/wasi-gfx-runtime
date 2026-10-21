use crate::surface::MainThreadSpawner;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::marker::PhantomData;
use std::sync::Arc;
use wasi_gfx::surface::surface_webgpu;
use wasi_webgpu_wasmtime::reexports::{wgpu_core, wgpu_types};
use wasmtime::{
    bail,
    component::{HasData, Resource},
};

wasmtime::component::bindgen!({
    world: "wasi-gfx:surface/webgpu-imports",
    require_store_data_send: true,
    imports: {
        default: trappable,
    },
    with: {
        "wasi-gfx:surface/surface": crate::surface::wasi_gfx::surface::surface,
        "wasi:webgpu/webgpu": wasi_webgpu_wasmtime::wasi::webgpu::webgpu,
        "wasi-gfx:surface/surface-webgpu.context": Context,
    },
});

// types
pub struct Context {
    pub(crate) surface: surface_webgpu::Surface,
    pub(crate) surface_id: wgpu_core::id::SurfaceId,
    pub(crate) configuration: Option<ContextConfiguration>,
}

pub(crate) struct ContextConfiguration {
    device: Resource<wasi_webgpu_wasmtime::Device>,
    // What configure passed to wgpu-core, so an Outdated or Lost acquire can
    // reconfigure the surface without the guest.
    surface_config: wgpu_types::SurfaceConfiguration<Vec<wgpu_types::TextureFormat>>,
}

// linker connection
pub fn add_to_linker<T>(l: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
where
    T: SurfaceWebgpuCtxView,
{
    wasi_gfx::surface::surface_webgpu::add_to_linker::<_, HasSurfaceWebgpu<T::Spawner>>(
        l,
        T::surface_webgpu_ctx,
    )?;
    Ok(())
}

pub trait SurfaceWebgpuCtxView: Send {
    /// Spawner used to run main-thread-only wgpu calls (e.g. surface creation).
    type Spawner: MainThreadSpawner;
    fn surface_webgpu_ctx(&mut self) -> SurfaceWebgpuCtx<'_, Self::Spawner>;
}

pub struct SurfaceWebgpuCtx<'a, S: MainThreadSpawner> {
    pub table: &'a mut wasmtime::component::ResourceTable,
    pub instance: &'a Arc<wasi_webgpu_wasmtime::reexports::wgpu_core::global::Global>,
    pub main_thread_spawner: &'a S,
}

struct HasSurfaceWebgpu<S>(PhantomData<S>);

impl<S: MainThreadSpawner> HasData for HasSurfaceWebgpu<S> {
    type Data<'a> = SurfaceWebgpuCtx<'a, S>;
}

// wasmtime trait impls
impl<'a, S: MainThreadSpawner> surface_webgpu::Host for SurfaceWebgpuCtx<'a, S> {}

impl<'a, S: MainThreadSpawner> surface_webgpu::HostContext for SurfaceWebgpuCtx<'a, S> {
    fn new(
        &mut self,
        surface: Resource<surface_webgpu::Surface>,
    ) -> wasmtime::Result<Resource<surface_webgpu::Context>> {
        let surface = self.table.get(&surface)?;
        let instance = Arc::clone(self.instance);

        let surface_id = futures::executor::block_on({
            let surface = surface.arc_clone();
            self.main_thread_spawner.spawn(move || {
                // SAFETY: The raw handles remain valid for the lifetime of the wgpu surface because
                // `Context` holds an `arc_clone()` of the surface alongside the `surface_id`.
                unsafe {
                    instance.instance_create_surface(
                        Some(surface.display_handle().unwrap().as_raw()),
                        surface.window_handle().unwrap().as_raw(),
                        None,
                    )
                }
            })
        })?;

        Ok(self.table.push(Context {
            surface: surface.arc_clone(),
            surface_id,
            configuration: None,
        })?)
    }

    fn configure(
        &mut self,
        context: Resource<surface_webgpu::Context>,
        configuration: surface_webgpu::ContextConfiguration,
    ) -> wasmtime::Result<()> {
        let device_id = *self.table.get(&configuration.device)?.device_id();

        let context = self.table.get_mut(&context)?;

        let surface_config = wgpu_types::SurfaceConfiguration {
            // present in WebGPU, same defaults https://www.w3.org/TR/webgpu/#dictdef-gpucanvasconfiguration
            format: configuration.format.into(),
            usage: configuration
                .usage
                .unwrap_or(
                    wasi_webgpu_wasmtime::wasi::webgpu::webgpu::GpuTextureUsage::RENDER_ATTACHMENT,
                )
                .try_into()
                .unwrap(),
            view_formats: configuration
                .view_formats
                .into_iter()
                .flatten()
                .map(|f| f.into())
                .collect(),
            alpha_mode: configuration
                .alpha_mode
                .unwrap_or(wasi_webgpu_wasmtime::wasi::webgpu::webgpu::GpuCanvasAlphaMode::Opaque)
                .into(),
            // not present in WebGPU
            width: context.surface.width(),
            height: context.surface.height(),
            present_mode: wgpu_types::PresentMode::default(),
            desired_maximum_frame_latency: 2,
        };
        let err = self
            .instance
            .surface_configure(context.surface_id, device_id, &surface_config);
        if let Some(err) = err {
            bail!("{err:#?}")
        }

        context.configuration = Some(ContextConfiguration {
            device: configuration.device,
            surface_config,
        });
        Ok(())
    }

    fn unconfigure(&mut self, context: Resource<surface_webgpu::Context>) -> wasmtime::Result<()> {
        let context = self.table.get_mut(&context)?;
        context.configuration = None;
        Ok(())
    }

    fn get_current_texture(
        &mut self,
        context: Resource<surface_webgpu::Context>,
    ) -> wasmtime::Result<Resource<surface_webgpu::GpuTexture>> {
        let context = self.table.get(&context)?;

        let Some(configuration) = &context.configuration else {
            bail!("Not configured")
        };

        let device = self.table.get(&configuration.device)?;

        // wgpu-core returns no texture with status Timeout, Occluded, Outdated
        // or Lost (Outdated after a resize, for example). Outdated or Lost:
        // reconfigure with the stored configuration at the surface's current
        // size and acquire once more. Timeout or Occluded: acquire once more.
        // If there is still no texture, return an error instead of panicking
        // the host.
        let instance = self.instance;
        let surface_id = context.surface_id;
        let first = match instance.surface_get_current_texture(surface_id, None) {
            Ok(output) => output,
            Err(err) => bail!("surface acquire failed: {err:#?}"),
        };
        let texture_id: wgpu_core::id::TextureId = match first.texture {
            Some(texture_id) => texture_id,
            None => {
                let status = first.status;
                match status {
                    wgpu_types::SurfaceStatus::Outdated | wgpu_types::SurfaceStatus::Lost => {
                        let mut config = configuration.surface_config.clone();
                        config.width = context.surface.width();
                        config.height = context.surface.height();
                        if let Some(err) =
                            instance.surface_configure(surface_id, *device.device_id(), &config)
                        {
                            bail!("surface acquire returned {status:?} with no texture and the reconfigure failed: {err:#?}")
                        }
                    }
                    wgpu_types::SurfaceStatus::Timeout | wgpu_types::SurfaceStatus::Occluded => {}
                    _ => bail!("surface acquire returned {status:?} with no texture"),
                }
                match instance.surface_get_current_texture(surface_id, None) {
                    Ok(wgpu_core::present::SurfaceOutput {
                        texture: Some(texture_id),
                        ..
                    }) => texture_id,
                    Ok(again) => bail!(
                        "surface acquire returned {:?} with no texture on the retry after {status:?}",
                        again.status
                    ),
                    Err(err) => {
                        bail!("surface acquire failed on the retry after {status:?}: {err:#?}")
                    }
                }
            }
        };

        // SAFETY: surface_get_current_texture will only give back a texture connected to the configured device.
        let texture = unsafe { device.connect_texture(texture_id) };

        Ok(self.table.push(texture)?)
    }

    fn present(&mut self, context: Resource<surface_webgpu::Context>) -> wasmtime::Result<()> {
        let surface_id = self.table.get(&context)?.surface_id;

        self.instance.surface_present(surface_id)?;
        Ok(())
    }

    fn drop(&mut self, surface: Resource<surface_webgpu::Context>) -> wasmtime::Result<()> {
        self.table.delete(surface)?;
        Ok(())
    }
}
