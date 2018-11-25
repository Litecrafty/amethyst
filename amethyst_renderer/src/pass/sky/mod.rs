use amethyst_assets::{AssetStorage, Loader};
use amethyst_core::{
    nalgebra as na,
    specs::prelude::{Component, Join, Read, ReadExpect, ReadStorage, VecStorage},
    transform::GlobalTransform,
};

use gfx::pso::buffer::{ElemStride, Element};
use gfx::texture::Kind;
use glsl_layout::*;

use crate::cam::{ActiveCamera, Camera};
use crate::error::Result;
use crate::formats::{ImageData, TextureData, TextureMetadata};
use crate::mesh::Mesh;
use crate::mtl::MaterialDefaults;
use crate::pass::util::get_camera;
use crate::pipe::pass::{Pass, PassData};
use crate::pipe::{Effect, NewEffect};
use crate::tex::{Texture, TextureHandle};
use crate::types::{Encoder, Factory};
use crate::vertex::{Attribute, AttributeFormat, Attributes, Position, VertexFormat, With};

const VERT_SRC: &[u8] = include_bytes!("../shaders/vertex/sky.glsl");
const FRAG_SRC: &[u8] = include_bytes!("../shaders/fragment/sky.glsl");

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Uniform)]
struct VertexArgs {
    proj: mat4,
    view: mat4,
}

/// Component to keep a handle to a cubemapped texture array
pub struct SkyBox {
    /// Handle to cubemapped texture array
    pub texture: TextureHandle,
}

impl Component for SkyBox {
    type Storage = VecStorage<Self>;
}

struct PosOnly {
    pub position: [f32; 3],
}

unsafe impl Pod for PosOnly {}

impl VertexFormat for PosOnly {
    const ATTRIBUTES: Attributes<'static> = &[(Position::NAME, <Self as With<Position>>::FORMAT)];
}

impl With<Position> for PosOnly {
    const FORMAT: AttributeFormat = Element {
        offset: 0,
        format: Position::FORMAT,
    };
}

/// Draws a sky box using cubemapped texture
pub struct DrawSkyBox {
    mesh: Option<Mesh>,
}

impl DrawSkyBox {
    /// Create instance of `DrawSkyBox` pass
    pub fn new() -> Self {
        DrawSkyBox { mesh: None }
    }
}

impl<'a> PassData<'a> for DrawSkyBox {
    type Data = (
        Option<Read<'a, ActiveCamera>>,
        ReadStorage<'a, Camera>,
        Read<'a, AssetStorage<Texture>>,
        ReadExpect<'a, MaterialDefaults>,
        ReadStorage<'a, GlobalTransform>,
        ReadStorage<'a, SkyBox>,
    );
}

impl Pass for DrawSkyBox {
    fn compile(&mut self, mut effect: NewEffect<'_>) -> Result<Effect> {
        let data: Vec<PosOnly> = SKYBOX_VERTICES
            .iter()
            .map(|v| PosOnly {
                position: v.clone(),
            }).collect();
        self.mesh = Some(Mesh::build(data).build(&mut effect.factory)?);
        use std::mem;
        effect
            .simple(VERT_SRC, FRAG_SRC)
            .with_raw_vertex_buffer(PosOnly::ATTRIBUTES, PosOnly::size() as ElemStride, 0)
            .with_raw_constant_buffer(
                "VertexArgs",
                mem::size_of::<<VertexArgs as Uniform>::Std140>(),
                1,
            ).with_texture("top")
            .with_output("color", None)
            .build()
    }

    fn apply<'a, 'b: 'a>(
        &'a mut self,
        encoder: &mut Encoder,
        effect: &mut Effect,
        _factory: Factory,
        (
            active,
            camera,
            tex_storage,
            material_defaults,
            global,
            skybox,
        ): <Self as PassData<'a>>::Data,
){
        let camera = get_camera(active, &camera, &global);
        let vertex_args = camera
            .as_ref()
            .map(|&(ref cam, ref transform)| {
                let proj: [[f32; 4]; 4] = cam.proj.into();
                let view: [[f32; 4]; 4] = transform.0.try_inverse().unwrap().into();
                VertexArgs {
                    proj: proj.into(),
                    view: view.into(),
                }
            }).unwrap_or_else(|| {
                let proj: [[f32; 4]; 4] = na::Matrix4::identity().into();
                let view: [[f32; 4]; 4] = na::Matrix4::identity().into();
                VertexArgs {
                    proj: proj.into(),
                    view: view.into(),
                }
            });

        for sky in (&skybox).join() {
            let mesh = self.mesh.as_ref().unwrap();

            //FIXME: it is probably not necessary to push the mesh to the GPU every frame. Loading
            //it once should be enough
            match mesh.buffer(PosOnly::ATTRIBUTES) {
                Some(vbuf) => effect.data.vertex_bufs.push(vbuf.clone()),
                None => {
                    effect.clear();
                    return;
                }
            }

            effect.update_constant_buffer("VertexArgs", &vertex_args.std140(), encoder);

            //TODO: Related to the above comment, the skybox texture most likely doesnt change
            //after scene setup. Having an option to access the Pass from within a system to update
            //the texture drawn would even elimitate the need for a seperate skybox component, the
            //texture could be stored in the pass directly.
            let texture = tex_storage
                .get(&sky.texture)
                .or_else(|| tex_storage.get(&material_defaults.0.albedo));
            effect.data.textures.push(texture.unwrap().view().clone());
            effect
                .data
                .samplers
                .push(texture.unwrap().sampler().clone());

            effect.draw(mesh.slice(), encoder);

            effect.clear();
        }
    }
}

/// Load a set of 6 textures as cubemapped texture array
pub fn load_cubemap<N>(
    names: [N; 6],
    size: u16,
    loader: &Loader,
    storage: &AssetStorage<Texture>,
) -> TextureHandle
where
    N: Into<String> + Copy,
{
    let data: [ImageData; 6] = [
        load_texture(names[0]),
        load_texture(names[1]),
        load_texture(names[2]),
        load_texture(names[3]),
        load_texture(names[4]),
        load_texture(names[5]),
    ];
    let meta = TextureMetadata::srgb().with_kind(Kind::Cube(size));

    let texture_data = TextureData::CubeImage(data, meta);
    loader.load_from_data(texture_data, (), storage)
}

fn load_texture<P: Into<String>>(path: P) -> ImageData {
    use image::load_from_memory;
    use image::DynamicImage;
    use std::fs::File;
    use std::io::Read;

    let mut data = Vec::new();
    let mut file = File::open(path.into()).unwrap();
    file.read_to_end(&mut data);

    load_from_memory(&data)
        .map(|image| {
            match image {
                DynamicImage::ImageRgba8(im) => im,
                _ => {
                    // TODO: Log performance warning.
                    image.to_rgba()
                }
            }
        }).map(|rgba| ImageData { rgba })
        .unwrap()
}

const SKYBOX_VERTICES: [[f32; 3]; 36] = [
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [-1.0, -1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, 1.0, 1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, -1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, -1.0],
    [1.0, -1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [-1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, -1.0, 1.0],
    [-1.0, -1.0, 1.0],
    [-1.0, 1.0, -1.0],
    [1.0, 1.0, -1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
];
