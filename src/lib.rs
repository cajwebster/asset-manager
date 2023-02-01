//! A simple asset loader that can load anything that implements the [`Asset`] trait
//!
//! Exaple
//! ```rust
//! # use {asset_manager::Asset, std::path::Path};
//! #
//! # struct Foo;
//! # impl Foo {
//! #     fn new(_bytes: &[u8]) -> Self {
//! #         Self
//! #     }
//! # }
//! #
//! impl Asset for Foo {
//!     type Resources = ();
//!     type Error = String;
//!
//!     fn load(path: impl AsRef<Path>, _resources: &Self::Resources) -> Result<Self, Self::Error> {
//!         let path = path.as_ref();
//!         match std::fs::read(path) {
//!             Ok(bytes) => Ok(Foo::new(&bytes)),
//!             Err(e) => Err(format!("Could not load Foo from {path:?}: {e}")),
//!         }
//!     }
//! }
//! ```

#![warn(missing_docs)]

use std::{
    collections::HashMap,
    hash::Hash,
    marker::PhantomData,
    path::{Path, PathBuf},
};

/// Trait for types that can be loaded as assets
pub trait Asset: Sized + 'static {
    /// What additional resources are required to load this asset.
    /// For assets that can be loaded using global resources, this can just be ```()```
    type Resources;
    /// What type of error is returned when the asset can't be loaded
    type Error: std::fmt::Display + std::fmt::Debug;

    /// Loads an asset from a given path
    /// # Errors
    /// This function returns an error if the asset could not be loaded
    fn load(path: impl AsRef<Path>, resources: &Self::Resources) -> Result<Self, Self::Error>;
}

#[derive(Clone)]
enum AssetState<E> {
    Loaded(usize),
    Unloaded(PathBuf),
    Error(PathBuf, E),
}

impl<E> PartialEq for AssetState<E> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Loaded(idx1), Self::Loaded(idx2)) => idx1 == idx2,
            (Self::Unloaded(path1), Self::Unloaded(path2))
            | (Self::Error(path1, _), Self::Error(path2, _)) => path1 == path2,
            _ => false,
        }
    }
}

impl<E> Eq for AssetState<E> {}

impl<E> Hash for AssetState<E> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Loaded(idx) => idx.hash(state),
            Self::Unloaded(path) | Self::Error(path, _) => path.hash(state),
        }
    }
}

/// A handle to an asset of type `T`. Used with an [`AssetManager<T>`].
pub struct AssetHandle<T: Asset> {
    state: AssetState<T::Error>,
    _asset: PhantomData<T>,
}

impl<T: Asset> AssetHandle<T> {
    /// Creates a new handle for an unloaded asset.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            state: AssetState::Unloaded(path.as_ref().into()),
            _asset: PhantomData,
        }
    }

    #[must_use]
    /// Returns the path to the asset if it is still unloaded, otherwise returns `None`.
    pub fn path(&self) -> Option<&Path> {
        match &self.state {
            AssetState::Unloaded(p) | AssetState::Error(p, _) => Some(p.as_path()),
            _ => None,
        }
    }

    #[must_use]
    /// Returns true if the asset hasn't been loaded yet.
    pub fn is_unloaded(&self) -> bool {
        matches!(self.state, AssetState::Unloaded(_))
    }

    #[must_use]
    /// Returns true if the asset has been succesfully loaded.
    pub fn is_loaded(&self) -> bool {
        matches!(self.state, AssetState::Loaded(_))
    }

    #[must_use]
    /// Returns true if the asset previously failed to load.
    pub fn is_err(&self) -> bool {
        matches!(self.state, AssetState::Error(_, _))
    }
}

impl<T: Asset> PartialEq for AssetHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

impl<T: Asset> Eq for AssetHandle<T> {}

impl<T: Asset> Hash for AssetHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.state.hash(state);
    }
}

impl<T: Asset> Clone for AssetHandle<T>
where
    T::Error: Clone,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            _asset: PhantomData,
        }
    }
}

/// Safety: Since handles don't actually contain the asset, it's safe to send
/// one to another thread even if the asset itself isn't `Send`.
unsafe impl<T: Asset> Send for AssetHandle<T> {}
/// Safety: Since handles don't actually contain the asset, it's safe to share
/// one between threads even if the asset itself isn't or `Sync`.
unsafe impl<T: Asset> Sync for AssetHandle<T> {}

/// A loader for [`Assets`](Asset)
pub struct AssetManager<T: Asset> {
    assets: Vec<T>,
    paths: HashMap<PathBuf, usize>,
}

impl<T: Asset> AssetManager<T> {
    /// Creates a new `AssetManager<T>` with no loaded assets
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a reference to a loaded asset
    pub fn get(&self, handle: &AssetHandle<T>) -> Option<&T> {
        match handle.state {
            AssetState::Loaded(idx) => Some(&self.assets[idx]),
            AssetState::Unloaded(_) | AssetState::Error(_, _) => None,
        }
    }

    /// Attempts to load an asset and updates the handle. Does nothing if the
    /// asset is already loaded.
    /// # Errors
    /// Returns an error if the [load](Asset::load) method returns an error.
    /// Returns `Ok` if the asset is already loaded, or if a previous attempt
    /// to load the asset failed.
    pub fn load<'a>(
        &mut self,
        handle: &'a mut AssetHandle<T>,
        resources: &T::Resources,
    ) -> Result<(), &'a T::Error> {
        match &handle.state {
            AssetState::Loaded(_) | AssetState::Error(_, _) => Ok(()),
            AssetState::Unloaded(path) => match self.paths.get(path) {
                Some(&idx) => {
                    handle.state = AssetState::Loaded(idx);
                    Ok(())
                }
                None => {
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
                            handle.state = AssetState::Error(path.clone(), e);
                            match &handle.state {
                                AssetState::Error(_, e) => Err(e),
                                _ => unreachable!(),
                            }
                        }
                    }
                }
            },
        }
    }
}

impl<T: Asset> Default for AssetManager<T> {
    fn default() -> Self {
        Self {
            assets: Vec::default(),
            paths: HashMap::default(),
        }
    }
}
