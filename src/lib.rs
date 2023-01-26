use std::{
    collections::HashMap,
    marker::PhantomData,
    path::{Path, PathBuf},
};

pub trait Asset: Sized + 'static {
    type Resources;
    type Error: std::fmt::Display + std::fmt::Debug + Clone;

    fn load(path: impl AsRef<Path>, resources: &Self::Resources) -> Result<Self, Self::Error>;
}

pub struct HandleID(usize);

enum AssetState {
    Loaded(usize),
    Unloaded(PathBuf),
    Error(PathBuf, String),
}

pub struct AssetHandle<T: Asset> {
    state: AssetState,
    _asset: PhantomData<T>,
}

impl<T: Asset> AssetHandle<T> {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            state: AssetState::Unloaded(path.as_ref().into()),
            _asset: PhantomData,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match &self.state {
            AssetState::Unloaded(p) => Some(p.as_path()),
            _ => None,
        }
    }

    pub fn is_err(&self) -> bool {
        matches!(self.state, AssetState::Error(_, _))
    }
}

unsafe impl<T: Asset> Send for AssetHandle<T> {}
unsafe impl<T: Asset> Sync for AssetHandle<T> {}

pub struct AssetManager<T: Asset> {
    assets: Vec<T>,
    paths: HashMap<PathBuf, usize>,
}

impl<T: Asset> AssetManager<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, handle: &AssetHandle<T>) -> Option<&T> {
        match handle.state {
            AssetState::Loaded(idx) => Some(&self.assets[idx]),
            AssetState::Unloaded(_) | AssetState::Error(_, _) => None,
        }
    }

    pub fn load(
        &mut self,
        handle: &mut AssetHandle<T>,
        resources: &T::Resources,
    ) -> Result<(), T::Error> {
        match &handle.state {
            AssetState::Loaded(_) => Ok(()),
            AssetState::Unloaded(path) => {
                log::debug!(
                    "Loading asset '{}' of type '{}'",
                    path.display(),
                    std::any::type_name::<T>()
                );
                let idx = self.assets.len();
                let loaded_asset = T::load(path, resources);
                match loaded_asset {
                    Ok(loaded_asset) => {
                        self.assets.push(loaded_asset);
                        self.paths.insert(path.clone(), idx);
                        handle.state = AssetState::Loaded(idx);
                        Ok(())
                    }
                    Err(e) => {
                        handle.state = AssetState::Error(path.clone(), format!("{e}"));
                        Err(e)
                    }
                }
            }
            AssetState::Error(_, _) => Ok(()),
        }
    }
}

impl<T: Asset> Default for AssetManager<T> {
    fn default() -> Self {
        Self {
            assets: Default::default(),
            paths: Default::default(),
        }
    }
}
