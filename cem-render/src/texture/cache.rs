use std::{
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        Weak,
    },
};

use bevy_ecs::resource::Resource;
use parking_lot::Mutex;

#[derive(Clone, Debug, Default, Resource)]
pub struct TextureCache {
    cache: Arc<Mutex<HashMap<PathBuf, Entry>>>,
}

impl TextureCache {
    fn get_entry(&self, path: &Path) -> GetEntry {
        let mut cache = self.cache.lock();
        if let Some(entry) = cache.get_mut(path) {
            match entry {
                Entry::Loading { receiver } => GetEntry::Loading(receiver.activate_cloned()),
                Entry::Loaded {
                    texture,
                    image_info,
                } => {
                    if let Some(texture) = texture.upgrade() {
                        GetEntry::Ready((texture, *image_info))
                    }
                    else {
                        let (sender, receiver) = async_broadcast::broadcast(1);
                        *entry = Entry::Loading {
                            receiver: receiver.deactivate(),
                        };
                        GetEntry::NotPresent(sender)
                    }
                }
            }
        }
        else {
            let (sender, receiver) = async_broadcast::broadcast(1);
            cache.insert(
                path.to_owned(),
                Entry::Loading {
                    receiver: receiver.deactivate(),
                },
            );
            GetEntry::NotPresent(sender)
        }
    }

    pub async fn get_or_insert<E, L>(
        &self,
        path: &Path,
        load: L,
    ) -> Result<(Arc<wgpu::Texture>, ImageInfo), E>
    where
        L: AsyncFnOnce() -> Result<(wgpu::Texture, ImageInfo), E>,
    {
        let get_entry = self.get_entry(path).wait().await;

        match get_entry {
            Ok(texture) => Ok(texture),
            Err(sender) => {
                // load the image. this is async because image loading might take a while. thus
                // we need to make sure the cache is not locked while doing so.
                let (texture, image_info) = load().await?;
                let texture = Arc::new(texture);

                {
                    // insert into cache
                    let mut cache = self.cache.lock();
                    let entry = cache.get_mut(path).unwrap();
                    *entry = Entry::Loaded {
                        texture: Arc::downgrade(&texture),
                        image_info,
                    };
                }

                // if others are trying to get this texture at the same time they will find it
                // in loading state and get the receiver
                match sender.try_broadcast((texture.clone(), image_info)) {
                    Ok(_) => {}
                    Err(async_broadcast::TrySendError::Full(_)) => panic!("channel can't be full"),
                    Err(async_broadcast::TrySendError::Closed(_)) => {
                        // this means nobody has a receiver for it and we just
                        // dropped the one in the cache
                    }
                    Err(async_broadcast::TrySendError::Inactive(_)) => {
                        // nobody has a receiver for this. (the only inactive
                        // one should be in the cache
                        // and we just dropped it).
                    }
                }

                Ok((texture, image_info))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImageInfo {
    pub original_color_type: image::ColorType,
}

#[derive(Debug)]
enum Entry {
    Loading {
        receiver: async_broadcast::InactiveReceiver<(Arc<wgpu::Texture>, ImageInfo)>,
    },
    Loaded {
        texture: Weak<wgpu::Texture>,
        image_info: ImageInfo,
    },
}

enum GetEntry {
    Ready((Arc<wgpu::Texture>, ImageInfo)),
    Loading(async_broadcast::Receiver<(Arc<wgpu::Texture>, ImageInfo)>),
    NotPresent(async_broadcast::Sender<(Arc<wgpu::Texture>, ImageInfo)>),
}

impl GetEntry {
    async fn wait(
        self,
    ) -> Result<
        (Arc<wgpu::Texture>, ImageInfo),
        async_broadcast::Sender<(Arc<wgpu::Texture>, ImageInfo)>,
    > {
        match self {
            GetEntry::Ready(texture) => Ok(texture),
            GetEntry::Loading(mut receiver) => {
                // todo: handle the error
                Ok(receiver.recv_direct().await.unwrap())
            }
            GetEntry::NotPresent(sender) => Err(sender),
        }
    }
}
