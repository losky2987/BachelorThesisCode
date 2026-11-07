use alloc::{format, vec, vec::Vec};
use concurrent::shm::{shm_attach, shm_detach, shm_open, shm_unlink};
use core::{
    ptr,
    sync::atomic::{AtomicU64, Ordering, fence},
};
use graphic::{
    bitmap::{Bitmap, ScalingMode},
    color::Color,
};
use syscall::return_vals::Errno::{self, EAGAIN, EINVAL, ENOMEM};
use terminal::println;

pub const FRAME_CACHE_MIN_SIZE: usize = 1024;
pub const LOGICAL_FRAME_SIZE: (u32, u32) = (1000, 750);
pub const MAX_FRAME_SIZE: (u32, u32) = (800, 600); // current qemu display size

const FRAME_CACHE_MAGIC: u32 = 0x4433_4643; // "D3FC"
const FRAME_CACHE_VERSION: u32 = 2;
const PIXEL_FORMAT_BGRA8888: u32 = 1;
const FRAME_CACHE_SLOT_COUNT: u32 = 2;

/// Versioned, self-describing shared-memory header.
///
/// Metadata is immutable for the lifetime of one named object. A resize
/// creates a new object, so only `publication` is concurrently modified.
#[repr(C)]
struct FrameCacheHeader {
    magic: u32,
    version: u32,
    header_size: u32,
    frame_type: u32,
    pixel_format: u32,
    width: u32,
    height: u32,
    stride_bytes: u32,
    slot_count: u32,
    reserved_0: u32,
    data_len: u64,
    owner_app_id: u64,
    window_id: u64,
    /// The process ID is the surface generation in the current process model.
    /// A replacement application attached to the same window gets a new ID.
    generation: u64,
    /// Zero means no complete frame. Otherwise bits 63..1 contain the
    /// monotonically increasing revision and bit 0 selects the active slot.
    /// Packing both fields prevents readers from observing a mismatched pair.
    publication: AtomicU64,
    reserved_1: u64,
}

const FRAME_CACHE_HEADER_SIZE: usize = core::mem::size_of::<FrameCacheHeader>();

#[derive(Clone, Copy)]
struct FrameCacheMetadata {
    magic: u32,
    version: u32,
    header_size: u32,
    frame_type: u32,
    pixel_format: u32,
    width: u32,
    height: u32,
    stride_bytes: u32,
    slot_count: u32,
    data_len: u64,
    owner_app_id: u64,
    window_id: u64,
    generation: u64,
}

pub struct FrameCache {
    owner_app_id: usize,
    window_id: usize,
    frame_type: FrameType,
    shm_id: usize,
    /// Persistent process-local mapping. Polling a frame revision must not
    /// attach/detach shared memory on every compositor iteration: those calls
    /// create and tear down VMAs/page tables and contend on the kernel frame
    /// allocator.
    shm_ptr: *mut u8,
    width: u32,
    height: u32,
    stride_bytes: u32,
    data_len: usize,
    unlink_on_drop: bool,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FrameType {
    // WindowFrame, // Deprecated, early design but not in use
    CanvasFrame,
    ComponentFrame,
}

impl FrameType {
    fn suffix(self) -> &'static str {
        match self {
            // FrameType::WindowFrame => "windowframe",
            FrameType::CanvasFrame => "canvasframe",
            FrameType::ComponentFrame => "componentframe",
        }
    }

    const fn code(self) -> u32 {
        match self {
            // FrameType::WindowFrame => 1,
            FrameType::CanvasFrame => 1,
            FrameType::ComponentFrame => 2,
        }
    }
}

impl FrameCache {
    pub fn new(owner_app_id: usize, window_id: usize, width: u32, height: u32, frame_type: FrameType) -> Self {
        Self::create_writer(owner_app_id, window_id, width, height, frame_type, "created")
    }

    /// Replace the writer-owned shared-memory object during a resize.
    ///
    /// Consuming the old cache prevents its `Drop` implementation from unlinking
    /// the newly-created object that reuses the same route name.
    pub fn recreate(mut self, width: u32, height: u32) -> Self {
        let name = Self::name(self.owner_app_id, self.window_id, self.frame_type);
        if self.unlink_on_drop {
            let _ = shm_unlink(&name);
            self.unlink_on_drop = false;
        }

        Self::create_writer(self.owner_app_id, self.window_id, width, height, self.frame_type, "recreated")
    }

    fn create_writer(owner_app_id: usize, window_id: usize, width: u32, height: u32, frame_type: FrameType, operation: &str) -> Self {
        let name = Self::name(owner_app_id, window_id, frame_type);
        let (width, height) = Self::scale_frame_size_to_max(width, height);
        assert!(width > 0 && height > 0, "FrameCache dimensions must be non-zero");
        let data_len = Self::checked_data_size(width, height).expect("FrameCache dimensions overflow");
        let cache_size = Self::checked_cache_size(width, height).expect("FrameCache size overflow");
        let stride_bytes = Self::checked_stride(width).expect("FrameCache stride overflow");
        let shm_id = match shm_open(&name, cache_size, true) {
            Ok(shm_id) => shm_id,
            Err(Errno::EEXIST) => {
                // Process and window IDs may restart after a full graphical
                // session. Replace a stale name before publishing v2 metadata.
                let _ = shm_unlink(&name);
                shm_open(&name, cache_size, true).expect(&format!("FrameCache replace failed on {}", name))
            }
            Err(error) => panic!("FrameCache open failed on {}: {:?}", name, error),
        };

        println!(
            "FrameCache {} for app: {}, window: {}, shm_id: {}, width: {}, height: {}, frame_type: {:?}",
            operation, owner_app_id, window_id, shm_id, width, height, frame_type,
        );

        let shm_ptr = shm_attach(shm_id, false).expect(&format!("FrameCache attach failed on {}", name));
        let cache = Self {
            owner_app_id,
            window_id,
            frame_type,
            shm_id,
            shm_ptr,
            width,
            height,
            stride_bytes,
            data_len,
            unlink_on_drop: true,
        };
        cache.write_header();
        cache
    }

    pub fn get_instance(owner_app_id: usize, window_id: usize, width: u32, height: u32, frame_type: FrameType) -> Self {
        Self::try_get_instance(owner_app_id, window_id, width, height, frame_type)
            .expect(&format!("FrameCache open failed for app {}, window {}", owner_app_id, window_id))
    }

    pub fn try_get_instance(owner_app_id: usize, window_id: usize, width: u32, height: u32, frame_type: FrameType) -> Result<Self, Errno> {
        let name = Self::name(owner_app_id, window_id, frame_type);
        let (width, height) = Self::scale_frame_size_to_max(width, height);
        if width == 0 || height == 0 {
            return Err(EINVAL);
        }

        let data_len = Self::checked_data_size(width, height).ok_or(ENOMEM)?;
        let cache_size = Self::checked_cache_size(width, height).ok_or(ENOMEM)?;
        let stride_bytes = Self::checked_stride(width).ok_or(ENOMEM)?;

        // The kernel rejects an open request larger than the actual object.
        // Passing the complete expected size therefore validates that copying
        // `data_len` bytes cannot overrun a truncated/stale shared object.
        let shm_id = shm_open(&name, cache_size, false)?;
        let shm_ptr = shm_attach(shm_id, true)?;
        let cache = Self {
            owner_app_id,
            window_id,
            frame_type,
            shm_id,
            shm_ptr,
            width,
            height,
            stride_bytes,
            data_len,
            unlink_on_drop: false,
        };

        if !cache.metadata_is_valid() {
            let _ = shm_detach(shm_ptr);
            core::mem::forget(cache);
            return Err(EINVAL);
        }

        Ok(cache)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride_bytes(&self) -> u32 {
        self.stride_bytes
    }

    pub fn data_len(&self) -> usize {
        self.data_len
    }

    pub fn write_data(&self, data: &[u8]) -> Result<(), Errno> {
        if data.len() != self.data_len {
            println!(
                "FrameCache rejected frame for app {}, window {}: expected {} bytes, received {}",
                self.owner_app_id,
                self.window_id,
                self.data_len,
                data.len(),
            );
            return Err(EINVAL);
        }
        if !self.metadata_is_valid() {
            return Err(EINVAL);
        }

        let publication = self.publication();
        let current = publication.load(Ordering::Relaxed);
        let active_slot = (current & 1) as usize;
        let target_slot = if current == 0 { 0 } else { active_slot ^ 1 };
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.data_ptr(target_slot), self.data_len);
        }

        let mut next_revision = (current >> 1).wrapping_add(1);
        if next_revision == 0 {
            next_revision = 1;
        }
        let next_publication = (next_revision << 1) | target_slot as u64;

        // A reader that observes this Release store through an Acquire load
        // also observes every pixel copied into the newly active slot.
        publication.store(next_publication, Ordering::Release);
        Ok(())
    }

    /// Return the revision of the latest complete frame. The Window Manager
    /// polls this shared atomic instead of waiting for an EventPipe notification.
    pub fn completed_revision(&self) -> Result<Option<u64>, Errno> {
        if !self.metadata_is_valid() {
            return Err(EINVAL);
        }

        let publication = self.publication().load(Ordering::Acquire);
        if publication == 0 {
            return Ok(None);
        }
        Ok(Some(publication >> 1))
    }

    pub fn read_data(&self) -> Result<Vec<u8>, Errno> {
        if !self.metadata_is_valid() {
            return Err(EINVAL);
        }

        let publication_before = self.publication().load(Ordering::Acquire);
        if publication_before == 0 {
            return Err(EAGAIN);
        }

        let active_slot = (publication_before & 1) as usize;
        let mut buffer = vec![0; self.data_len];
        unsafe {
            ptr::copy_nonoverlapping(self.data_ptr(active_slot), buffer.as_mut_ptr(), self.data_len);
        }

        fence(Ordering::SeqCst);
        if self.publication().load(Ordering::Relaxed) != publication_before {
            return Err(EAGAIN);
        }
        Ok(buffer)
    }

    pub fn write_bitmap(&self, bitmap: &Bitmap) -> Result<(), Errno> {
        let scaled;
        let bitmap = if bitmap.width != self.width || bitmap.height != self.height {
            scaled = bitmap.scale(self.width, self.height, ScalingMode::NearestNeighbor);
            &scaled
        } else {
            bitmap
        };
        let Some(byte_len) = bitmap.data.len().checked_mul(core::mem::size_of::<Color>()) else {
            return Err(ENOMEM);
        };
        let bytes = unsafe { core::slice::from_raw_parts(bitmap.data.as_ptr() as *const u8, byte_len) };

        self.write_data(bytes)
    }

    /// Read the cached frame directly into an existing bitmap buffer.
    ///
    /// This avoids allocating a temporary `Vec<u8>` and a new `Vec<Color>` on
    /// every compositor pass. A concurrent publication is reported as
    /// `EAGAIN`; callers that want to retain their old frame can use
    /// `read_bitmap_snapshot_into` directly.
    pub fn read_bitmap_into(&self, bitmap: &mut Bitmap) -> Result<(), Errno> {
        match self.read_bitmap_snapshot_into(bitmap)? {
            Some(_) => Ok(()),
            None => Err(EAGAIN),
        }
    }

    /// Import one coherent bitmap snapshot.
    ///
    /// `None` means that no complete frame exists yet or that the producer
    /// changed the revision while the pixels were copied. In that case the
    /// caller must keep displaying its previous accepted frame.
    pub fn read_bitmap_snapshot_into(&self, bitmap: &mut Bitmap) -> Result<Option<u64>, Errno> {
        if !self.metadata_is_valid() {
            return Err(EINVAL);
        }

        let publication_before = self.publication().load(Ordering::Acquire);
        if publication_before == 0 {
            return Ok(None);
        }

        let active_slot = (publication_before & 1) as usize;
        self.copy_bitmap_into(bitmap, active_slot);

        // Every pixel load must complete before the publication word is
        // sampled again. A mismatch means the producer advanced far enough to
        // reuse this slot, so the imported pixels are discarded.
        fence(Ordering::SeqCst);
        let publication_after = self.publication().load(Ordering::Relaxed);
        if publication_before != publication_after {
            return Ok(None);
        }

        Ok(Some(publication_after >> 1))
    }

    fn copy_bitmap_into(&self, bitmap: &mut Bitmap, slot: usize) {
        let pixel_count = self.data_len / core::mem::size_of::<Color>();

        bitmap.width = self.width;
        bitmap.height = self.height;
        if bitmap.data.len() != pixel_count {
            bitmap.data.resize(pixel_count, Color::new(0, 0, 0, 0));
        }

        unsafe {
            ptr::copy_nonoverlapping(self.data_ptr(slot), bitmap.data.as_mut_ptr() as *mut u8, self.data_len);
        }
    }

    pub fn read_bitmap_scaled_into(&self, bitmap: &mut Bitmap, target_width: u32, target_height: u32, mode: ScalingMode) -> Result<(), Errno> {
        if target_width == self.width && target_height == self.height {
            return self.read_bitmap_into(bitmap);
        }

        let source = self.read_bitmap()?;
        let scaled = source.scale(target_width, target_height, mode);
        bitmap.width = scaled.width;
        bitmap.height = scaled.height;
        bitmap.data = scaled.data;
        Ok(())
    }

    pub fn read_bitmap(&self) -> Result<Bitmap, Errno> {
        let mut bitmap = Bitmap {
            width: self.width,
            height: self.height,
            data: vec![Color::new(0, 0, 0, 0); self.data_len / core::mem::size_of::<Color>()],
        };

        self.read_bitmap_into(&mut bitmap)?;
        Ok(bitmap)
    }

    fn name(owner_app_id: usize, window_id: usize, frame_type: FrameType) -> alloc::string::String {
        format!("gui_{}_{}_{}_v{}", owner_app_id, window_id, frame_type.suffix(), FRAME_CACHE_VERSION,)
    }

    pub fn scale_frame_size_to_max(width: u32, height: u32) -> (u32, u32) {
        if width == 0 || height == 0 {
            return (width, height);
        }

        let scale = (f64::from(MAX_FRAME_SIZE.0) / f64::from(width))
            .min(f64::from(MAX_FRAME_SIZE.1) / f64::from(height))
            .min(1.0);

        (((f64::from(width) * scale) as u32).max(1), ((f64::from(height) * scale) as u32).max(1))
    }

    fn write_header(&self) {
        let header = FrameCacheHeader {
            magic: FRAME_CACHE_MAGIC,
            version: FRAME_CACHE_VERSION,
            header_size: FRAME_CACHE_HEADER_SIZE as u32,
            frame_type: self.frame_type.code(),
            pixel_format: PIXEL_FORMAT_BGRA8888,
            width: self.width,
            height: self.height,
            stride_bytes: self.stride_bytes,
            slot_count: FRAME_CACHE_SLOT_COUNT,
            reserved_0: 0,
            data_len: self.data_len as u64,
            owner_app_id: self.owner_app_id as u64,
            window_id: self.window_id as u64,
            generation: self.owner_app_id as u64,
            publication: AtomicU64::new(0),
            reserved_1: 0,
        };

        unsafe {
            ptr::write(self.shm_ptr.cast::<FrameCacheHeader>(), header);
        }
    }

    fn metadata_is_valid(&self) -> bool {
        let metadata = Self::read_metadata(self.shm_ptr);
        metadata.magic == FRAME_CACHE_MAGIC
            && metadata.version == FRAME_CACHE_VERSION
            && metadata.header_size as usize == FRAME_CACHE_HEADER_SIZE
            && metadata.frame_type == self.frame_type.code()
            && metadata.pixel_format == PIXEL_FORMAT_BGRA8888
            && metadata.width == self.width
            && metadata.height == self.height
            && metadata.stride_bytes == self.stride_bytes
            && metadata.slot_count == FRAME_CACHE_SLOT_COUNT
            && metadata.data_len == self.data_len as u64
            && metadata.owner_app_id == self.owner_app_id as u64
            && metadata.window_id == self.window_id as u64
            && metadata.generation == self.owner_app_id as u64
    }

    fn read_metadata(shm_ptr: *mut u8) -> FrameCacheMetadata {
        let source = shm_ptr.cast::<FrameCacheHeader>();
        unsafe {
            FrameCacheMetadata {
                magic: ptr::read_volatile(ptr::addr_of!((*source).magic)),
                version: ptr::read_volatile(ptr::addr_of!((*source).version)),
                header_size: ptr::read_volatile(ptr::addr_of!((*source).header_size)),
                frame_type: ptr::read_volatile(ptr::addr_of!((*source).frame_type)),
                pixel_format: ptr::read_volatile(ptr::addr_of!((*source).pixel_format)),
                width: ptr::read_volatile(ptr::addr_of!((*source).width)),
                height: ptr::read_volatile(ptr::addr_of!((*source).height)),
                stride_bytes: ptr::read_volatile(ptr::addr_of!((*source).stride_bytes)),
                slot_count: ptr::read_volatile(ptr::addr_of!((*source).slot_count)),
                data_len: ptr::read_volatile(ptr::addr_of!((*source).data_len)),
                owner_app_id: ptr::read_volatile(ptr::addr_of!((*source).owner_app_id)),
                window_id: ptr::read_volatile(ptr::addr_of!((*source).window_id)),
                generation: ptr::read_volatile(ptr::addr_of!((*source).generation)),
            }
        }
    }

    fn publication(&self) -> &AtomicU64 {
        unsafe { &(*self.shm_ptr.cast::<FrameCacheHeader>()).publication }
    }

    fn data_ptr(&self, slot: usize) -> *mut u8 {
        debug_assert!(slot < FRAME_CACHE_SLOT_COUNT as usize);
        unsafe { self.shm_ptr.add(FRAME_CACHE_HEADER_SIZE + slot * self.data_len) }
    }

    fn checked_stride(width: u32) -> Option<u32> {
        width.checked_mul(core::mem::size_of::<Color>() as u32)
    }

    fn checked_data_size(width: u32, height: u32) -> Option<usize> {
        (width as usize).checked_mul(height as usize)?.checked_mul(core::mem::size_of::<Color>())
    }

    fn checked_cache_size(width: u32, height: u32) -> Option<usize> {
        Self::checked_data_size(width, height)?
            .checked_mul(FRAME_CACHE_SLOT_COUNT as usize)?
            .checked_add(FRAME_CACHE_HEADER_SIZE)
            .map(|bytes| bytes.max(FRAME_CACHE_MIN_SIZE))
    }
}

impl Drop for FrameCache {
    fn drop(&mut self) {
        if self.unlink_on_drop {
            let _ = shm_unlink(&Self::name(self.owner_app_id, self.window_id, self.frame_type));
        }
        if let Err(error) = shm_detach(self.shm_ptr) {
            println!("FrameCache cannot detach shm_id {}, because {:?}", self.shm_id, error);
        }
    }
}
