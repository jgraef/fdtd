use bevy_ecs::{
    resource::Resource,
    system::{
        Commands,
        Res,
        ResMut,
        SystemParam,
    },
};
use cem_util::wgpu::{
    ImageTextureExt,
    UnsupportedColorSpace,
    buffer::{
        SubmitOnDrop,
        WriteStagingBelt,
        WriteStagingCommit,
        WriteStagingTransaction,
    },
    create_texture,
    create_texture_from_linsrgba,
};
use nalgebra::Vector2;
use palette::LinSrgba;

use crate::renderer::{
    renderer::{
        Renderer,
        SharedRenderer,
    },
    texture::channel::{
        TextureReceiver,
        UndecidedTextureSender,
    },
};

#[derive(derive_more::Debug, SystemParam)]
pub struct RenderResourceManager<'w, 's> {
    renderer: Res<'w, SharedRenderer>,
    transaction: Option<ResMut<'w, Transaction>>,
    #[debug(skip)]
    commands: Commands<'w, 's>,
}

impl<'w, 's> RenderResourceManager<'w, 's> {
    fn with_transaction<R>(&mut self, f: impl FnOnce(&Renderer, &mut Transaction) -> R) -> R {
        if let Some(mut transaction) = self.transaction.as_mut() {
            f(&self.renderer, &mut transaction)
        }
        else {
            let mut transaction = Transaction::new(&self.renderer.0);
            let output = f(&self.renderer, &mut transaction);
            self.commands.insert_resource(transaction);
            output
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.renderer.wgpu_context.device
    }

    pub fn create_texture(
        &mut self,
        size: &Vector2<u32>,
        usage: wgpu::TextureUsages,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> wgpu::Texture {
        create_texture(
            size,
            usage,
            format,
            label,
            &self.renderer.wgpu_context.device,
        )
    }

    pub fn create_texture_from_image(
        &mut self,
        image: &image::RgbaImage,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> Result<wgpu::Texture, UnsupportedColorSpace> {
        self.with_transaction(|renderer, transaction| {
            image.create_texture(
                usage,
                label,
                &renderer.wgpu_context.device,
                &mut transaction.write_staging,
            )
        })
    }

    pub fn create_texture_from_color(
        &mut self,
        color: LinSrgba<u8>,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> wgpu::Texture {
        self.with_transaction(|renderer, transaction| {
            create_texture_from_linsrgba(
                color,
                usage,
                label,
                &renderer.wgpu_context.device,
                &mut transaction.write_staging,
            )
        })
    }

    pub fn create_texture_channel(
        &mut self,
        _size: &Vector2<u32>,
        _usage: wgpu::TextureUsages,
        _label: &str,
    ) -> (UndecidedTextureSender, TextureReceiver) {
        // todo: bevy-migrate

        /*let texture = self.create_texture(size, usage, wgpu::TextureFormat::Rgba8Unorm, label);

        texture_channel(
            Arc::new(TextureAndView::from_texture(texture, label)),
            *size,
            self.command_sender.clone(),
        )*/
        todo!();
    }
}

#[derive(Debug, Resource)]
struct Transaction {
    write_staging: SubmitOnDrop<
        WriteStagingTransaction<WriteStagingBelt, wgpu::Device, wgpu::CommandEncoder>,
        wgpu::Queue,
    >,
}

impl Transaction {
    fn new(renderer: &Renderer) -> Self {
        Self {
            write_staging: SubmitOnDrop::new(
                WriteStagingTransaction::new(
                    renderer.wgpu_context.staging_pool.belt(),
                    renderer.wgpu_context.device.clone(),
                    renderer.wgpu_context.device.create_command_encoder(
                        &wgpu::CommandEncoderDescriptor {
                            label: Some("render/resource_manager/transaction"),
                        },
                    ),
                ),
                renderer.wgpu_context.queue.clone(),
            ),
        }
    }

    pub fn commit(self) {
        // this also submits it to the queue
        self.write_staging.commit();
    }

    pub fn discard(self) {
        self.write_staging.discard();
    }
}
