//! Engine-neutral bounded thumbnail rendering. [ADR-005, ADR-009, ADR-011]

use engine_api::rasterize::{Rasterize, RasterizeRequest, TileRect};
use protocol::utility_thumbnails::{
    decode_thumbnail_request, encode_thumbnail_result, ThumbnailCodecError, ThumbnailResult,
};

/// Failure while rendering a bounded utility thumbnail.
#[derive(Debug, PartialEq, Eq)]
pub enum ThumbnailRenderError {
    /// Request or result metadata failed trust-boundary validation.
    Codec(ThumbnailCodecError),
    /// The provided shared-memory output grant is too small.
    OutputTooSmall,
    /// The engine returned dimensions or pixels inconsistent with the request.
    OutputMismatch,
    /// The engine capability returned an error.
    Engine(String),
}

/// Render one thumbnail through the engine capability into a bounded output grant.
pub fn render_thumbnail(
    engine: &dyn Rasterize,
    request_bytes: &[u8],
    output: &mut [u8],
) -> Result<Vec<u8>, ThumbnailRenderError> {
    let request = decode_thumbnail_request(request_bytes).map_err(ThumbnailRenderError::Codec)?;
    let byte_length = usize::try_from(request.width * request.height * 4)
        .map_err(|_| ThumbnailRenderError::OutputMismatch)?;
    if output.len() < byte_length {
        return Err(ThumbnailRenderError::OutputTooSmall);
    }
    let rendered = engine
        .rasterize(&RasterizeRequest {
            page_index: request.page,
            rect: TileRect {
                x: 0,
                y: 0,
                w: request.width,
                h: request.height,
            },
            scale: request.scale,
        })
        .map_err(|error| ThumbnailRenderError::Engine(error.to_string()))?;
    if rendered.width != request.width
        || rendered.height != request.height
        || rendered.rgba_pixels.len() != byte_length
    {
        return Err(ThumbnailRenderError::OutputMismatch);
    }
    output[..byte_length].copy_from_slice(&rendered.rgba_pixels);
    encode_thumbnail_result(&ThumbnailResult {
        page: request.page,
        width: request.width,
        height: request.height,
        byte_length: byte_length as u32,
        generation: request.generation,
        revision: request.revision,
    })
    .map_err(ThumbnailRenderError::Codec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::rasterize::{RasterizeError, RasterizeRequest, TileOutput};
    use protocol::utility_thumbnails::{
        decode_thumbnail_result, encode_thumbnail_request, ThumbnailRequest,
    };

    struct FixtureRasterizer;

    impl Rasterize for FixtureRasterizer {
        fn rasterize(&self, request: &RasterizeRequest) -> Result<TileOutput, RasterizeError> {
            Ok(TileOutput {
                rgba_pixels: vec![7; (request.rect.w * request.rect.h * 4) as usize],
                width: request.rect.w,
                height: request.rect.h,
            })
        }

        fn page_count(&self) -> u32 {
            10
        }
    }

    #[test]
    fn thumbnail_handler_writes_bounded_pixels_through_capability_trait() {
        let request = ThumbnailRequest {
            page: 2,
            width: 16,
            height: 24,
            scale: 0.25,
            generation: 4,
            revision: 8,
        };
        let mut output = vec![0; 16 * 24 * 4];

        let encoded = render_thumbnail(
            &FixtureRasterizer,
            &encode_thumbnail_request(&request).unwrap(),
            &mut output,
        )
        .unwrap();
        let result = decode_thumbnail_result(&encoded).unwrap();

        assert!(output.iter().all(|byte| *byte == 7));
        assert_eq!(result.page, 2);
        assert!(result.is_current(4, 8));
    }

    #[test]
    fn thumbnail_handler_rejects_undersized_output_grant() {
        let request = ThumbnailRequest {
            page: 0,
            width: 16,
            height: 16,
            scale: 1.0,
            generation: 0,
            revision: 0,
        };
        let mut output = vec![0; 16 * 16 * 4 - 1];

        assert_eq!(
            render_thumbnail(
                &FixtureRasterizer,
                &encode_thumbnail_request(&request).unwrap(),
                &mut output,
            ),
            Err(ThumbnailRenderError::OutputTooSmall)
        );
    }
}
