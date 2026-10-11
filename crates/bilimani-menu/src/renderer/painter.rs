//! Small fixed-function D3D9 painter for egui meshes; no shader compiler DLL.
#![allow(unsafe_op_in_unsafe_fn)]
use egui::{
    TextureId,
    epaint::{ClippedPrimitive, Primitive, textures::TexturesDelta},
};
use std::collections::HashMap;
use windows::{
    Win32::{Foundation::RECT, Graphics::Direct3D9::*},
    core::{Error, Result},
};
#[repr(C)]
struct Vertex {
    x: f32,
    y: f32,
    z: f32,
    rhw: f32,
    color: u32,
    u: f32,
    v: f32,
}
struct Texture {
    image: egui::ColorImage,
    gpu: Option<IDirect3DTexture9>,
}
#[derive(Default)]
pub struct Painter {
    textures: HashMap<TextureId, Texture>,
}
impl Painter {
    pub fn invalidate(&mut self) {
        for texture in self.textures.values_mut() {
            texture.gpu = None;
        }
    }
    /// # Safety
    /// Call on the serialized device thread inside a scene, with the target bound.
    /// The caller must restore graphics state and invalidate textures before Reset.
    pub unsafe fn paint(
        &mut self,
        device: &IDirect3DDevice9,
        delta: TexturesDelta,
        primitives: &[ClippedPrimitive],
        scale: f32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        for (id, update) in delta.set {
            let egui::ImageData::Color(image) = update.image;
            if let Some([x, y]) = update.pos {
                let Some(texture) = self.textures.get_mut(&id) else {
                    continue;
                };
                if x + image.width() > texture.image.width()
                    || y + image.height() > texture.image.height()
                {
                    return Err(Error::from_hresult(windows::core::HRESULT(
                        0x80070057u32 as i32,
                    )));
                }
                for row in 0..image.height() {
                    let start = (row + y) * texture.image.width() + x;
                    texture.image.pixels[start..start + image.width()].copy_from_slice(
                        &image.pixels[row * image.width()..(row + 1) * image.width()],
                    );
                }
                texture.gpu = None;
            } else {
                self.textures.insert(
                    id,
                    Texture {
                        image: (*image).clone(),
                        gpu: None,
                    },
                );
            }
        }
        let result = self.draw(device, primitives, scale, width, height);
        for id in delta.free {
            self.textures.remove(&id);
        }
        result
    }
    unsafe fn draw(
        &mut self,
        device: &IDirect3DDevice9,
        primitives: &[ClippedPrimitive],
        scale: f32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        device.SetVertexShader(None)?;
        device.SetPixelShader(None)?;
        device.SetFVF(D3DFVF_XYZRHW | D3DFVF_DIFFUSE | D3DFVF_TEX1)?;
        device.SetViewport(&D3DVIEWPORT9 {
            X: 0,
            Y: 0,
            Width: width,
            Height: height,
            MinZ: 0.0,
            MaxZ: 1.0,
        })?;
        for (state, value) in [
            (D3DRS_ZENABLE, 0),
            (D3DRS_ZWRITEENABLE, 0),
            (D3DRS_ALPHATESTENABLE, 0),
            (D3DRS_CULLMODE, D3DCULL_NONE.0 as u32),
            (D3DRS_LIGHTING, 0),
            (D3DRS_FOGENABLE, 0),
            (D3DRS_STENCILENABLE, 0),
            (D3DRS_SCISSORTESTENABLE, 1),
            (D3DRS_ALPHABLENDENABLE, 1),
            (D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32),
            (D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32),
            (D3DRS_BLENDOP, D3DBLENDOP_ADD.0 as u32),
            (D3DRS_SEPARATEALPHABLENDENABLE, 0),
            (D3DRS_COLORWRITEENABLE, 15),
            (D3DRS_SRGBWRITEENABLE, 0),
            (D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32),
            (D3DRS_CLIPPING, 1),
        ] {
            device.SetRenderState(state, value)?;
        }
        for (state, value) in [
            (D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32),
            (D3DTSS_COLORARG1, D3DTA_TEXTURE),
            (D3DTSS_COLORARG2, D3DTA_DIFFUSE),
            (D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32),
            (D3DTSS_ALPHAARG1, D3DTA_TEXTURE),
            (D3DTSS_ALPHAARG2, D3DTA_DIFFUSE),
            (D3DTSS_TEXCOORDINDEX, 0),
            (D3DTSS_TEXTURETRANSFORMFLAGS, 0),
        ] {
            device.SetTextureStageState(0, state, value)?;
        }
        device.SetTextureStageState(1, D3DTSS_COLOROP, D3DTOP_DISABLE.0 as u32)?;
        device.SetTextureStageState(1, D3DTSS_ALPHAOP, D3DTOP_DISABLE.0 as u32)?;
        for (state, value) in [
            (D3DSAMP_MINFILTER, D3DTEXF_LINEAR.0 as u32),
            (D3DSAMP_MAGFILTER, D3DTEXF_LINEAR.0 as u32),
            (D3DSAMP_MIPFILTER, D3DTEXF_NONE.0 as u32),
            (D3DSAMP_ADDRESSU, D3DTADDRESS_CLAMP.0 as u32),
            (D3DSAMP_ADDRESSV, D3DTADDRESS_CLAMP.0 as u32),
            (D3DSAMP_SRGBTEXTURE, 0),
        ] {
            device.SetSamplerState(0, state, value)?;
        }
        for primitive in primitives {
            let Primitive::Mesh(mesh) = &primitive.primitive else {
                continue;
            };
            if mesh.indices.is_empty() {
                continue;
            }
            let Some(texture) = self.textures.get_mut(&mesh.texture_id) else {
                continue;
            };
            if texture.gpu.is_none() {
                texture.upload(device)?;
            }
            let r = primitive.clip_rect;
            let clip = RECT {
                left: (r.min.x * scale).floor().clamp(0.0, width as f32) as i32,
                top: (r.min.y * scale).floor().clamp(0.0, height as f32) as i32,
                right: (r.max.x * scale).ceil().clamp(0.0, width as f32) as i32,
                bottom: (r.max.y * scale).ceil().clamp(0.0, height as f32) as i32,
            };
            if clip.left >= clip.right || clip.top >= clip.bottom {
                continue;
            }
            device.SetScissorRect(&clip)?;
            device.SetTexture(0, texture.gpu.as_ref().unwrap())?;
            let vertices: Vec<_> = mesh
                .vertices
                .iter()
                .map(|v| Vertex {
                    x: v.pos.x * scale - 0.5,
                    y: v.pos.y * scale - 0.5,
                    z: 0.0,
                    rhw: 1.0,
                    color: bgra(v.color),
                    u: v.uv.x,
                    v: v.uv.y,
                })
                .collect();
            device.DrawIndexedPrimitiveUP(
                D3DPT_TRIANGLELIST,
                0,
                vertices.len() as u32,
                (mesh.indices.len() / 3) as u32,
                mesh.indices.as_ptr().cast(),
                D3DFMT_INDEX32,
                vertices.as_ptr().cast(),
                std::mem::size_of::<Vertex>() as u32,
            )?;
        }
        Ok(())
    }
}
fn bgra(c: egui::Color32) -> u32 {
    u32::from_le_bytes([c.b(), c.g(), c.r(), c.a()])
}
impl Texture {
    unsafe fn upload(&mut self, device: &IDirect3DDevice9) -> Result<()> {
        let mut texture = None;
        device.CreateTexture(
            self.image.width() as u32,
            self.image.height() as u32,
            1,
            D3DUSAGE_DYNAMIC as u32,
            D3DFMT_A8R8G8B8,
            D3DPOOL_DEFAULT,
            &mut texture,
            std::ptr::null_mut(),
        )?;
        let texture = texture.unwrap();
        let mut locked = D3DLOCKED_RECT::default();
        texture.LockRect(0, &mut locked, std::ptr::null(), D3DLOCK_DISCARD as u32)?;
        for y in 0..self.image.height() {
            let row =
                (locked.pBits as *mut u8).offset(y as isize * locked.Pitch as isize) as *mut u32;
            for x in 0..self.image.width() {
                row.add(x)
                    .write_unaligned(bgra(self.image.pixels[y * self.image.width() + x]));
            }
        }
        texture.UnlockRect(0)?;
        self.gpu = Some(texture);
        Ok(())
    }
}
