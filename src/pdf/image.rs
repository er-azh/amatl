//! Template images as image XObjects.

use pdf_writer::{Name, Pdf, Ref};

use crate::template::Image;

/// `QPdfEnginePrivate::writeImage` for an image and its mask: the JPEG as is, masks deflated.
pub fn write_image(pdf: &mut Pdf, alloc: &mut impl FnMut() -> Ref, img: &Image) -> Ref {
    use crate::template::Mask;
    use pdf_writer::Filter;
    let id = alloc();
    let mask = img.mask.as_ref().map(|m| {
        let mid = alloc();
        let deflate = |d: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(d, 6);
        match m {
            Mask::Soft(a) => {
                let data = deflate(a);
                let mut x = pdf.image_xobject(mid, &data);
                x.filter(Filter::FlateDecode);
                x.width(img.w as i32).height(img.h as i32);
                x.color_space_name(Name(b"DeviceGray"))
                    .bits_per_component(8);
            }
            Mask::Stencil(b) => {
                let data = deflate(b);
                let mut x = pdf.image_xobject(mid, &data);
                x.filter(Filter::FlateDecode);
                x.width(img.w as i32).height(img.h as i32);
                x.image_mask(true).decode([1.0, 0.0]);
            }
        }
        (matches!(m, Mask::Soft(_)), mid)
    });
    let mut x = pdf.image_xobject(id, &img.jpeg);
    x.filter(Filter::DctDecode);
    x.width(img.w as i32).height(img.h as i32);
    x.color_space_name(Name(b"DeviceRGB")).bits_per_component(8);
    match mask {
        Some((true, m)) => {
            x.s_mask(m);
        }
        Some((false, m)) => {
            x.stencil_mask(m);
        }
        None => {}
    }
    id
}
