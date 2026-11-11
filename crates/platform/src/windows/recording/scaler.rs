use bediz_core::capture::Pixels;
use bediz_core::geometry::Rect;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_USAGE_DEFAULT, D3D11_VIEWPORT,
    ID3D11Buffer, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::core::{Error, Result, s};

use crate::windows::gpu;
use crate::windows::shader;

// Samples are averaged after the sRGB encoding, as SDR desktop values are, so HDR and SDR match.
const SHADER: &str = r"
cbuffer Area : register(b0) { float2 origin; float2 stride; float scale; float linear_input; };
Texture2D<float4> frame : register(t0);

float4 cover(uint id : SV_VertexID) : SV_Position {
    float2 corner = float2((id << 1) & 2, id & 2);
    return float4(corner * float2(2, -2) + float2(-1, 1), 0, 1);
}

float3 shown(int2 at) {
    float3 value = frame.Load(int3(at, 0)).rgb;
    if (linear_input < 0.5) {
        return value;
    }
    float3 light = saturate(value * scale);
    float3 curve = 1.055 * pow(light, 1 / 2.4) - 0.055;
    return lerp(light * 12.92, curve, step(0.0031308, light));
}

float4 shrink(float4 position : SV_Position) : SV_Target {
    float2 start = origin + floor(position.xy) * stride;
    float2 end = start + stride;
    int2 first = int2(floor(start));
    int2 last = int2(ceil(end)) - 1;
    float3 sum = 0;
    float total = 0;
    [loop] for (int y = first.y; y <= last.y; y++) {
        float tall = min(end.y, y + 1) - max(start.y, y);
        [loop] for (int x = first.x; x <= last.x; x++) {
            float weight = (min(end.x, x + 1) - max(start.x, x)) * tall;
            sum += shown(int2(x, y)) * weight;
            total += weight;
        }
    }
    return float4(sum / max(total, 0.000001), 1);
}
";

pub struct Scaler {
    device: ID3D11Device,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    constants: ID3D11Buffer,
    target: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
    area: Rect,
    width: u32,
    height: u32,
}

impl Scaler {
    pub fn new(device: &ID3D11Device, area: Rect, width: u32, height: u32) -> Result<Self> {
        let vertex_code = shader::compile(SHADER, s!("cover"), s!("vs_4_0"))?;
        let pixel_code = shader::compile(SHADER, s!("shrink"), s!("ps_4_0"))?;
        let target = gpu::image_texture(device, width, height)?;
        let mut vertex = None;
        let mut pixel = None;
        let mut constants = None;
        let mut view = None;
        let description = D3D11_BUFFER_DESC {
            ByteWidth: 32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        unsafe {
            device.CreateVertexShader(&vertex_code, None, Some(&mut vertex))?;
            device.CreatePixelShader(&pixel_code, None, Some(&mut pixel))?;
            device.CreateBuffer(&description, None, Some(&mut constants))?;
            device.CreateRenderTargetView(&target, None, Some(&mut view))?;
        }
        Ok(Self {
            device: device.clone(),
            vertex: vertex.ok_or_else(|| Error::from(E_POINTER))?,
            pixel: pixel.ok_or_else(|| Error::from(E_POINTER))?,
            constants: constants.ok_or_else(|| Error::from(E_POINTER))?,
            target,
            view: view.ok_or_else(|| Error::from(E_POINTER))?,
            area,
            width,
            height,
        })
    }

    pub fn target(&self) -> &ID3D11Texture2D {
        &self.target
    }

    pub fn draw(
        &self,
        context: &ID3D11DeviceContext,
        frame: &ID3D11Texture2D,
        pixels: Pixels,
    ) -> Result<()> {
        let mut source = None;
        unsafe {
            self.device
                .CreateShaderResourceView(frame, None, Some(&mut source))?;
        }
        let linear = if matches!(pixels, Pixels::Bgra8) {
            0.0
        } else {
            1.0
        };
        let constants = [
            self.area.left as f32,
            self.area.top as f32,
            self.area.width() as f32 / self.width as f32,
            self.area.height() as f32 / self.height as f32,
            pixels.scale(),
            linear,
            0.0,
            0.0,
        ];
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.width as f32,
            Height: self.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            context.ClearState();
            context.UpdateSubresource(&self.constants, 0, None, constants.as_ptr().cast(), 0, 0);
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[source]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            context.RSSetViewports(Some(&[viewport]));
            context.OMSetRenderTargets(
                Some(&[Some(self.view.clone())]),
                None::<&ID3D11DepthStencilView>,
            );
            context.Draw(3, 0);
            context.ClearState();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

    use super::*;

    const ONE: u16 = 0x3C00;
    const HALF: u16 = 0x3800;
    const TWO_AND_A_HALF: u16 = 0x4100;

    struct Frame<'a> {
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        pixel_bytes: u32,
        data: &'a [u8],
    }

    fn bgra(width: u32, height: u32, data: &[u8]) -> Frame<'_> {
        Frame {
            width,
            height,
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            pixel_bytes: 4,
            data,
        }
    }

    fn halves(values: &[u16]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn description(width: u32, height: u32, format: DXGI_FORMAT) -> D3D11_TEXTURE2D_DESC {
        D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            ..Default::default()
        }
    }

    fn scaled(frame: &Frame, area: Rect, width: u32, height: u32, pixels: Pixels) -> Vec<u8> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap();
        let (device, context): (ID3D11Device, ID3D11DeviceContext) =
            (device.unwrap(), context.unwrap());
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: frame.data.as_ptr().cast(),
            SysMemPitch: frame.width * frame.pixel_bytes,
            SysMemSlicePitch: 0,
        };
        let mut source = None;
        let mut staging = None;
        unsafe {
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..description(frame.width, frame.height, frame.format)
                    },
                    Some(&initial),
                    Some(&mut source),
                )
                .unwrap();
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..description(width, height, DXGI_FORMAT_B8G8R8A8_UNORM)
                    },
                    None,
                    Some(&mut staging),
                )
                .unwrap();
        }
        let (source, staging) = (source.unwrap(), staging.unwrap());
        let scaler = Scaler::new(&device, area, width, height).unwrap();
        scaler.draw(&context, &source, pixels).unwrap();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context.CopyResource(&staging, scaler.target());
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
        }
        let mut bgra = Vec::new();
        for row in 0..height as usize {
            let start = unsafe {
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize)
            };
            bgra.extend_from_slice(unsafe {
                std::slice::from_raw_parts(start, width as usize * 4)
            });
        }
        unsafe { context.Unmap(&staging, 0) };
        bgra
    }

    fn assert_near(actual: &[u8], expected: &[u8]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "{actual:?} is not {expected:?}"
            );
        }
    }

    #[test]
    fn each_block_of_four_pixels_becomes_their_average() {
        let top = [
            0, 0, 0, 255, 255, 255, 255, 255, 100, 0, 0, 255, 100, 0, 0, 255,
        ];
        let bottom = [
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 200, 255, 0, 0, 200, 255,
        ];
        let data = [top, bottom].concat();
        let pixels = scaled(
            &bgra(4, 2, &data),
            Rect::new(0, 0, 4, 2),
            2,
            1,
            Pixels::Bgra8,
        );
        assert_near(&pixels, &[128, 128, 128, 255, 50, 0, 100, 255]);
    }

    #[test]
    fn only_the_recorded_area_is_cut_from_the_frame() {
        let data = [
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        let pixels = scaled(
            &bgra(4, 1, &data),
            Rect::new(1, 0, 3, 1),
            2,
            1,
            Pixels::Bgra8,
        );
        assert_near(&pixels, &[40, 50, 60, 255, 70, 80, 90, 255]);
    }

    #[test]
    fn linear_light_is_encoded_as_srgb_in_bgra_order() {
        let data = halves(&[ONE, 0, 0, ONE, HALF, HALF, HALF, ONE]);
        let frame = Frame {
            width: 2,
            height: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            pixel_bytes: 8,
            data: &data,
        };
        let pixels = scaled(&frame, Rect::new(0, 0, 2, 1), 2, 1, Pixels::Linear);
        assert_near(&pixels, &[0, 0, 255, 255, 188, 188, 188, 255]);
    }

    #[test]
    fn hdr_pixels_are_averaged_after_encoding_like_sdr_ones() {
        let data = halves(&[ONE, ONE, ONE, ONE, 0, 0, 0, ONE]);
        let frame = Frame {
            width: 2,
            height: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            pixel_bytes: 8,
            data: &data,
        };
        let pixels = scaled(&frame, Rect::new(0, 0, 2, 1), 1, 1, Pixels::Linear);
        assert_near(&pixels, &[128, 128, 128, 255]);
    }

    #[test]
    fn sdr_white_becomes_white_under_hdr() {
        let data = halves(&[TWO_AND_A_HALF, TWO_AND_A_HALF, TWO_AND_A_HALF, ONE]);
        let frame = Frame {
            width: 1,
            height: 1,
            format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            pixel_bytes: 8,
            data: &data,
        };
        let hdr = Pixels::Hdr {
            sdr_white_level: NonZeroU32::new(2500).unwrap(),
        };
        let pixels = scaled(&frame, Rect::new(0, 0, 1, 1), 1, 1, hdr);
        assert_near(&pixels, &[255, 255, 255, 255]);
    }
}
