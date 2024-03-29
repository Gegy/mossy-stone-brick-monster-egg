use std::ops::Deref;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub trait Persistable: Serialize + DeserializeOwned + Default + Clone + Eq {}

impl<T: Serialize + DeserializeOwned + Default + Clone + Eq> Persistable for T {}

pub struct Persistent<T: Persistable> {
    path: PathBuf,
    inner: T,
}

impl<T: Persistable> Persistent<T> {
    pub async fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        let inner = if path.exists() {
            let mut file = File::open(&path).await.expect("failed to open file");

            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await.expect("failed to load file");

            serde_json::from_slice(&bytes).expect("failed to deserialize")
        } else {
            T::default()
        };

        Persistent { path, inner }
    }

    #[inline]
    pub async fn write<F, R>(&mut self, f: F) -> R
        where F: FnOnce(&mut T) -> R
    {
        let previous = self.inner.clone();
        let result = f(&mut self.inner);

        // our state didn't change, don't bother trying to write the config
        if previous == self.inner {
            return result;
        }

        let mut file = File::create(&self.path).await.expect("failed to create file");

        let bytes = serde_json::to_vec(&self.inner).expect("failed to serialize");
        file.write_all(&bytes).await.expect("failed to write to file");

        result
    }

    #[inline]
    pub fn read(&self) -> &T {
        &self.inner
    }
}

impl<T: Persistable> Deref for Persistent<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.inner
    }
}
