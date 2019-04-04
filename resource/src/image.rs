//! Image usage, format, kind, extent, creation-info and wrappers.

pub use gfx_hal::image::*;

use {
    crate::{
        escape::Handle,
        memory::{Block, Heaps, MemoryBlock, MemoryUsage},
        util::{device_owned, Device, DeviceId},
    },
    gfx_hal::{format, Backend, Device as _},
    relevant::Relevant,
};

/// Image info.
#[derive(Clone, Copy, Debug)]
pub struct ImageInfo {
    /// Kind of the image.
    pub kind: Kind,

    /// Image mip-level count.
    pub levels: Level,

    /// Image format.
    pub format: format::Format,

    /// Image tiling mode.
    pub tiling: Tiling,

    /// Image view capabilities.
    pub view_caps: ViewCapabilities,

    /// Image usage flags.
    pub usage: Usage,
}

/// Generic image resource wrapper.
///
/// # Parameters
///
/// `B` - raw image type.
#[derive(Debug)]
pub struct Image<B: Backend> {
    device: DeviceId,
    raw: B::Image,
    block: Option<MemoryBlock<B>>,
    info: ImageInfo,
    relevant: Relevant,
}

device_owned!(Image<B>);

impl<B> Image<B>
where
    B: Backend,
{
    /// Create image.
    ///
    /// # Safety
    ///
    /// `info` must match information about raw image.
    /// `block` if provided must be the one bound to the raw image.
    /// `terminal` will receive image and memory block upon drop, it must free image and memory properly.
    ///
    pub unsafe fn create(
        device: &Device<B>,
        heaps: &mut Heaps<B>,
        info: ImageInfo,
        memory_usage: impl MemoryUsage,
    ) -> Result<Self, failure::Error> {
        assert!(
            info.levels <= info.kind.num_levels(),
            "Number of mip leves ({}) cannot be greater than {} for given kind {:?}",
            info.levels,
            info.kind.num_levels(),
            info.kind,
        );

        log::trace!("{:#?}@{:#?}", info, memory_usage);

        let mut img = device.create_image(
            info.kind,
            info.levels,
            info.format,
            info.tiling,
            info.usage,
            info.view_caps,
        )?;
        let reqs = device.get_image_requirements(&img);
        let block = heaps.allocate(
            device,
            reqs.type_mask as u32,
            memory_usage,
            reqs.size,
            reqs.alignment,
        )?;

        device.bind_image_memory(block.memory(), block.range().start, &mut img)?;

        Ok(Image {
            device: device.id(),
            raw: img,
            block: Some(block),
            info,
            relevant: Relevant,
        })
    }

    /// Create image handler for swapchain image.
    pub unsafe fn create_from_swapchain(device: DeviceId, info: ImageInfo, raw: B::Image) -> Self {
        Image {
            device,
            raw,
            block: None,
            info,
            relevant: Relevant,
        }
    }

    /// Destroy image resource.
    pub unsafe fn dispose(self, device: &Device<B>, heaps: &mut Heaps<B>) {
        self.assert_device_owner(device);
        device.destroy_image(self.raw);
        self.block.map(|block| heaps.free(device, block));
        self.relevant.dispose();
    }

    /// Drop image wrapper for swapchain image.
    pub unsafe fn dispose_swapchain_image(self, device: DeviceId) {
        assert_eq!(self.device_id(), device);
        assert!(self.block.is_none());
        self.relevant.dispose();
    }

    /// Get reference for raw image resource.
    pub fn raw(&self) -> &B::Image {
        &self.raw
    }

    /// Get mutable reference for raw image resource.
    pub unsafe fn raw_mut(&mut self) -> &mut B::Image {
        &mut self.raw
    }

    /// Get reference to memory block occupied by image.
    pub fn block(&self) -> Option<&MemoryBlock<B>> {
        self.block.as_ref()
    }

    /// Get mutable reference to memory block occupied by image.
    pub unsafe fn block_mut(&mut self) -> Option<&mut MemoryBlock<B>> {
        self.block.as_mut()
    }

    /// Get image info.
    pub fn info(&self) -> &ImageInfo {
        &self.info
    }

    /// Get [`Kind`] of the image.
    ///
    /// [`Kind`]: ../gfx-hal/image/struct.Kind.html
    pub fn kind(&self) -> Kind {
        self.info.kind
    }

    /// Get [`Format`] of the image.
    ///
    /// [`Format`]: ../gfx-hal/format/struct.Format.html
    pub fn format(&self) -> format::Format {
        self.info.format
    }

    /// Get levels count of the image.
    pub fn levels(&self) -> u8 {
        self.info.levels
    }

    /// Get layers count of the image.
    pub fn layers(&self) -> u16 {
        self.info.kind.num_layers()
    }
}

// Image view info
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct ImageViewInfo {
    pub view_kind: ViewKind,
    pub format: format::Format,
    pub swizzle: format::Swizzle,
    pub range: SubresourceRange,
}

/// Generic image view resource wrapper.
#[doc(hidden)]
#[derive(Debug)]
pub struct ImageView<B: Backend> {
    raw: B::ImageView,
    image: Handle<Image<B>>,
    info: ImageViewInfo,
    relevant: Relevant,
}

device_owned!(ImageView<B> @ |view: &Self| view.image.device_id());

impl<B> ImageView<B>
where
    B: Backend,
{
    /// Create an image view.
    pub fn create(
        device: &Device<B>,
        info: ImageViewInfo,
        image: Handle<Image<B>>,
    ) -> Result<Self, failure::Error> {
        log::trace!("{:#?}@{:#?}", info, image);

        image.assert_device_owner(device);

        assert!(match_kind(
            image.kind(),
            info.view_kind,
            image.info().view_caps
        ));

        let view = unsafe {
            device.create_image_view(
                image.raw(),
                info.view_kind,
                info.format,
                info.swizzle,
                SubresourceRange {
                    aspects: info.range.aspects.clone(),
                    layers: info.range.layers.clone(),
                    levels: info.range.levels.clone(),
                },
            )
        }?;

        Ok(ImageView {
            raw: view,
            image,
            info,
            relevant: Relevant,
        })
    }

    /// Destroy image view resource.
    pub unsafe fn dispose(self, device: &Device<B>) {
        device.destroy_image_view(self.raw);
        drop(self.image);
        self.relevant.dispose();
    }

    /// Get reference to raw image view resoruce.
    pub fn raw(&self) -> &B::ImageView {
        &self.raw
    }

    /// Get mutable reference to raw image view resoruce.
    pub unsafe fn raw_mut(&mut self) -> &mut B::ImageView {
        &mut self.raw
    }

    /// Get image view info.
    pub fn info(&self) -> &ImageViewInfo {
        &self.info
    }

    /// Get image of this view.
    pub fn image(&self) -> &Handle<Image<B>> {
        &self.image
    }
}

fn match_kind(kind: Kind, view_kind: ViewKind, view_caps: ViewCapabilities) -> bool {
    match kind {
        Kind::D1(..) => match view_kind {
            ViewKind::D1 | ViewKind::D1Array => true,
            _ => false,
        },
        Kind::D2(..) => match view_kind {
            ViewKind::D2 | ViewKind::D2Array => true,
            _ => false,
        },
        Kind::D3(..) => {
            if view_caps == ViewCapabilities::KIND_2D_ARRAY {
                if view_kind == ViewKind::D2 {
                    true
                } else if view_kind == ViewKind::D2Array {
                    true
                } else {
                    false
                }
            } else if view_kind == ViewKind::D3 {
                true
            } else {
                false
            }
        }
    }
}