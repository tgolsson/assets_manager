use crate::{
    Asset,
    AssetCache,
    AssetError,
    AssetRef,
    utils::{RwLock, RwLockReadGuard},
};

use std::{
    iter::FusedIterator,
    io,
    fmt,
    fs,
    marker::PhantomData,
    path::Path,
};

#[inline]
pub(crate) fn extension_of(path: &Path) -> Option<&str> {
    match path.extension() {
        Some(ext) => ext.to_str(),
        None => Some(""),
    }
}

#[inline]
fn has_extension(path: &Path, ext: &[&str]) -> bool {
    match extension_of(path) {
        Some(file_ext) => ext.contains(&file_ext),
        None => false,
    }
}

#[inline]
pub(crate) fn id_push(id: &mut String, name: &str) {
    if !id.is_empty() {
        id.push('.');
    }
    id.push_str(name);
}

struct StringList {
    list: RwLock<Vec<Box<str>>>,
}

impl From<Vec<Box<str>>> for StringList {
    #[inline]
    fn from(vec: Vec<Box<str>>) -> Self {
        Self {
            list: RwLock::new(vec),
        }
    }
}

impl<'a> IntoIterator for &'a StringList {
    type Item = &'a str;
    type IntoIter = StringIter<'a>;

    #[inline]
    fn into_iter(self) -> StringIter<'a> {
        let guard = self.list.read();
        let current = guard.as_ptr();
        let end = unsafe { current.add(guard.len()) };

        StringIter {
            current,
            end,

            _guard: guard,
        }
    }
}

struct StringIter<'a> {
    current: *const Box<str>,
    end: *const Box<str>,

    _guard: RwLockReadGuard<'a, Vec<Box<str>>>,
}

impl<'a> Iterator for StringIter<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.end {
            None
        } else {
            unsafe {
                let string = &*self.current;
                self.current = self.current.offset(1);
                Some(string)
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for StringIter<'_> {
    #[inline]
    fn len(&self) -> usize {
        let diff = (self.end as usize) - (self.current as usize);
        diff / std::mem::size_of::<Box<str>>()
    }
}

impl FusedIterator for StringIter<'_> {}

pub(crate) struct CachedDir {
    assets: Box<StringList>,
}

impl CachedDir {
    pub fn load<A: Asset>(cache: &AssetCache, path: &Path, id: &str) -> Result<Self, io::Error> {
        let entries = fs::read_dir(path)?;

        let mut loaded = Vec::new();

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();

                if !has_extension(&path, A::EXTENSIONS) {
                    continue;
                }

                let name = match path.file_stem().and_then(|n| n.to_str()) {
                    Some(name) => name,
                    None => continue,
                };

                if path.is_file() {
                    let mut this_id = id.to_owned();
                    id_push(&mut this_id, name);

                    let _ = cache.load::<A>(&this_id);
                    loaded.push(this_id.into());
                }
            }
        }

        Ok(Self {
            assets: Box::new(loaded.into()),
        })
    }

    #[cfg(feature = "hot-reloading")]
    #[inline]
    pub fn contains(&self, id: &str) -> bool {
        self.assets.into_iter().any(|s| s == id)
    }

    #[cfg(feature = "hot-reloading")]
    #[inline]
    pub fn add(&self, id: Box<str>) {
        let mut list = self.assets.list.write();
        list.push(id);
    }

    #[cfg(feature = "hot-reloading")]
    #[inline]
    pub fn remove(&self, id: &str) {
        let mut list = self.assets.list.write();

        if let Some(pos) = list.iter().position(|s| s.as_ref() == id) {
            list.remove(pos);
        }
    }

    #[inline]
    pub unsafe fn read<'a, A>(&self, cache: &'a AssetCache) -> DirReader<'a, A> {
        DirReader {
            cache,
            assets: &*(&*self.assets as *const StringList),
            _marker: PhantomData,
        }
    }
}

impl fmt::Debug for CachedDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&*self.assets.list.read()).finish()
    }
}

/// A reference to all assets in a directory.
///
/// This type provides methods to iterates over theses assets.
///
/// When [hot-reloading] is used, added/removed files will be added/removed from
/// this structure.
///
/// This structure can be obtained by calling [`AssetCache::load_dir`].
///
/// [`AssetCache::load_dir`]: struct.AssetCache.html#method.load_dir
/// [hot-reloading]: struct.AssetCache.html#method.hot_reload
pub struct DirReader<'a, A> {
    cache: &'a AssetCache,
    assets: &'a StringList,
    _marker: PhantomData<&'a A>,
}

impl<A> Clone for DirReader<'_, A> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for DirReader<'_, A> {}

impl<'a, A: Asset> DirReader<'a, A> {
    /// An iterator over successfully loaded assets in a directory.
    ///
    /// This iterator yields each asset that was successfully loaded. It is
    /// garantied to perform no I/O. This is the method you want to use most of
    /// the time.
    ///
    /// Note that if an asset is [removed from the cache], it won't be returned
    /// by this iterator until it is cached again.
    ///
    /// [removed from the cache]: struct.AssetCache.html#method.remove
    #[inline]
    pub fn iter(&self) -> ReadDir<'a, A> {
        ReadDir {
            cache: self.cache,
            iter: self.assets.into_iter(),
            _marker: PhantomData,
        }
    }

    /// An iterator over all assets in a directory.
    ///
    /// This iterator yields the id asset of each asset in a directory, with the
    /// result of its last loading from the cache. It will happily try to reload
    /// any asset that is not in the cache (e.g. that previously failed to load
    /// or was removed).
    #[inline]
    pub fn iter_all(&self) -> ReadAllDir<'a, A> {
        ReadAllDir {
            cache: self.cache,
            iter: self.assets.into_iter(),
            _marker: PhantomData,
        }
    }
}

impl<'a, A> IntoIterator for &DirReader<'a, A>
where
    A: Asset,
{
    type Item = AssetRef<'a, A>;
    type IntoIter = ReadDir<'a, A>;

    /// Equivalent to [`iter`](#method.iter).
    #[inline]
    fn into_iter(self) -> ReadDir<'a, A> {
        self.iter()
    }
}

/// An iterator over successfully loaded assets in a directory.
///
/// This iterator yields each asset that was successfully loaded.
///
/// It can be obtained by calling [`DirReader::iter`].
///
/// [`DirReader::iter`]: struct.DirReader.html#method.iter
pub struct ReadDir<'a, A> {
    cache: &'a AssetCache,
    iter: StringIter<'a>,
    _marker: PhantomData<&'a A>,
}

impl<'a, A> Iterator for ReadDir<'a, A>
where
    A: Asset,
{
    type Item = AssetRef<'a, A>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.iter.next()?;

            if let asset @ Some(_) = self.cache.load_cached(id) {
                break asset;
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.iter.size_hint().1)
    }
}

impl<A> FusedIterator for ReadDir<'_, A> where A: Asset {}

/// An iterator over all assets in a directory.
///
/// This iterator yields the id asset of each asset in a directory, with the
/// result of its loading from the cache.
///
/// It can be obtained by calling [`DirReader::iter_all`].
///
/// [`DirReader::iter_all`]: struct.DirReader.html#method.iter_all
pub struct ReadAllDir<'a, A> {
    cache: &'a AssetCache,
    iter: StringIter<'a>,
    _marker: PhantomData<&'a A>,
}

impl<'a, A> Iterator for ReadAllDir<'a, A>
where
    A: Asset,
{
    type Item = (&'a str, Result<AssetRef<'a, A>, AssetError<A>>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.iter.next()?;
        Some((id, self.cache.load(id)))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<A> ExactSizeIterator for ReadAllDir<'_, A>
where
    A: Asset,
{
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<A> FusedIterator for ReadAllDir<'_, A> where A: Asset {}

impl<A> fmt::Debug for DirReader<'_, A>
where
    A: fmt::Debug + Asset,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<A> fmt::Debug for ReadDir<'_, A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadDir").finish()
    }
}

impl<A> fmt::Debug for ReadAllDir<'_, A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadAllDir").finish()
    }
}
