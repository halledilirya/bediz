use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIEWPORT, ID3D11Buffer, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext,
    ID3D11PixelShader, ID3D11Texture2D, ID3D11VertexShader,
};
use windows::core::{Error, Result, s};

use crate::windows::shader;

// Highlights above SDR white are clipped on purpose: an adaptive tone mapper would shift SDR content.
const SHADER: &str = r"
cbuffer Conversion : register(b0) { float scale; };
Texture2D<float4> frame : register(t0);

float4 cover(uint id : SV_VertexID) : SV_Position {
    float2 corner = float2((id << 1) & 2, id & 2);
    return float4(corner * float2(2, -2) + float2(-1, 1), 0, 1);
}

float4 encode(float4 position : SV_Position) : SV_Target {
    float3 light = saturate(frame.Load(int3(position.xy, 0)).rgb * scale);
    float3 curve = 1.055 * pow(light, 1 / 2.4) - 0.055;
    return float4(lerp(light * 12.92, curve, step(0.0031308, light)), 1);
}
";

pub struct Converter {
    device: ID3D11Device,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    constants: ID3D11Buffer,
}

impl Converter {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex_code = shader::compile(SHADER, s!("cover"), s!("vs_4_0"))?;
        let pixel_code = shader::compile(SHADER, s!("encode"), s!("ps_4_0"))?;
        let mut vertex = None;
        let mut pixel = None;
        let mut constants = None;
        let description = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        unsafe {
            device.CreateVertexShader(&vertex_code, None, Some(&mut vertex))?;
            device.CreatePixelShader(&pixel_code, None, Some(&mut pixel))?;
            device.CreateBuffer(&description, None, Some(&mut constants))?;
        }
        Ok(Self {
            device: device.clone(),
            vertex: vertex.ok_or_else(|| Error::from(E_POINTER))?,
            pixel: pixel.ok_or_else(|| Error::from(E_POINTER))?,
            constants: constants.ok_or_else(|| Error::from(E_POINTER))?,
        })
    }

    pub fn convert(
        &self,
        context: &ID3D11DeviceContext,
        target: &ID3D11Texture2D,
        frame: &ID3D11Texture2D,
        scale: f32,
    ) -> Result<()> {
        let mut view = None;
        let mut source = None;
        unsafe {
            self.device
                .CreateRenderTargetView(target, None, Some(&mut view))?;
            self.device
                .CreateShaderResourceView(frame, None, Some(&mut source))?;
        }
        let mut description = D3D11_TEXTURE2D_DESC::default();
        unsafe { target.GetDesc(&mut description) };
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: description.Width as f32,
            Height: description.Height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let constants = [scale, 0.0, 0.0, 0.0];
        unsafe {
            context.ClearState();
            context.UpdateSubresource(&self.constants, 0, None, constants.as_ptr().cast(), 0, 0);
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[source]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            context.RSSetViewports(Some(&[viewport]));
            context.OMSetRenderTargets(Some(&[view]), None::<&ID3D11DepthStencilView>);
            context.Draw(3, 0);
            context.ClearState();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA, D3D11_USAGE_STAGING, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIAdapter;

    use super::*;

    const ONE: u16 = 0x3C00;
    const ONE_AND_A_QUARTER: u16 = 0x3D00;
    const TWO_AND_A_HALF: u16 = 0x4100;
    const FIVE: u16 = 0x4500;
    const MINUS_ONE: u16 = 0xBC00;

    fn description(width: u32, format: DXGI_FORMAT) -> D3D11_TEXTURE2D_DESC {
        D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: 1,
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

    fn convert(halves: &[u16], scale: f32) -> Vec<u8> {
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
        let width = (halves.len() / 4) as u32;
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: halves.as_ptr().cast(),
            SysMemPitch: width * 8,
            SysMemSlicePitch: 0,
        };
        let mut frame = None;
        let mut target = None;
        let mut staging = None;
        unsafe {
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..description(width, DXGI_FORMAT_R16G16B16A16_FLOAT)
                    },
                    Some(&initial),
                    Some(&mut frame),
                )
                .unwrap();
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                        ..description(width, DXGI_FORMAT_B8G8R8A8_UNORM)
                    },
                    None,
                    Some(&mut target),
                )
                .unwrap();
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..description(width, DXGI_FORMAT_B8G8R8A8_UNORM)
                    },
                    None,
                    Some(&mut staging),
                )
                .unwrap();
        }
        let (frame, target, staging) = (frame.unwrap(), target.unwrap(), staging.unwrap());
        Converter::new(&device)
            .unwrap()
            .convert(&context, &target, &frame, scale)
            .unwrap();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            context.CopyResource(&staging, &target);
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
        }
        let bgra =
            unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), width as usize * 4) }
                .to_vec();
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
    fn linear_pixels_are_encoded_as_srgb_in_bgra_order() {
        let pixels = convert(
            &[ONE, 0, 0, ONE, 0, 0, ONE, ONE, 0x3800, 0x3800, 0x3800, ONE],
            1.0,
        );
        assert_near(
            &pixels,
            &[0, 0, 255, 255, 255, 0, 0, 255, 188, 188, 188, 255],
        );
    }

    #[test]
    fn sdr_white_becomes_white_and_brighter_light_clips() {
        let pixels = convert(
            &[
                TWO_AND_A_HALF,
                TWO_AND_A_HALF,
                TWO_AND_A_HALF,
                ONE,
                ONE_AND_A_QUARTER,
                ONE_AND_A_QUARTER,
                ONE_AND_A_QUARTER,
                ONE,
                TWO_AND_A_HALF,
                FIVE,
                MINUS_ONE,
                ONE,
            ],
            0.4,
        );
        assert_near(
            &pixels,
            &[255, 255, 255, 255, 188, 188, 188, 255, 0, 255, 255, 255],
        );
    }
}
