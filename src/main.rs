use pollster::FutureExt;
use wgpu::{Device, Queue, Texture};

fn get_device_and_queue() -> (Device, Queue) {
    let instance = wgpu::Instance::new(wgpu::Backends::all());
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptionsBase {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }).block_on()
        .ok_or(anyhow::anyhow!("Couldn't create the adapter"))?;
    return adapter.request_device(&Default::default(), None).block_on()?;
}

fn get_texture(label: &str, usage: wgpu::TextureUsages) -> Texture {
    return device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: usage,
    });
}

fn main() -> anyhow::Result<()> {
    let (device, queue) = get_device_and_queue();

    let input_image = image::load_from_memory(include_bytes!("sushi.png"))?.to_rgba8();
    let (width, height) = input_image.dimensions();

    let texture_size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1
    };

    let input_texture = get_texture("input texture", wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let output_texture = get_texture("output texture", wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::STORAGE_BINDING);

    queue.write_texture(
        input_texture.as_image_copy(),
        bytemuck::cast_slice(input_image.as_raw()),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: std::num::NonZeroU32::new(4 * width),
            rows_per_image: None,
        },
        texture_size,
    );

    let shader = device.create_shader_module(&wgpu::ShaderModuleDescriptor{
        label: Some("Grayscale shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/grayscale.wgsl").into()),
    });

    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Grayscale pipeline"),
        layout: None,
        module: &shader,
        entry_point: "grayscale_main",
    });

    let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &input_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(
                    &output_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(
        &wgpu::CommandEncoderDescriptor{label:None}
    );

    {
        let (dispatch_width, dispatch_height) =
            compute_work_group_count((texture_size.width, texture_size.height), (16, 16));
        let mut compute_pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("Grayscale pass") });

        compute_pass.set_pipeline(&pipeline);
        compute_pass.set_bind_group(0, &texture_bind_group, &[]);
        compute_pass.dispatch(dispatch_width, dispatch_height, 1);
    }

    let padded_bytes_per_row= padded_bytes_per_row(width);
    let unpadded_bytes_per_row = width as usize * 4;

    let output_buffer_size =
        padded_bytes_per_row as u64 * height as u64 * std::mem::size_of::<u8>() as u64;
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            aspect: wgpu::TextureAspect::All,
            texture: &output_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
        },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(padded_bytes_per_row as u32),
                rows_per_image: std::num::NonZeroU32::new(height),
            }
        },
        texture_size,
    );

    queue.submit(Some(encoder.finish()));

    let buffer_slice = output_buffer.slice(..);
    let mapping = buffer_slice.map_async(wgpu::MapMode::Read);

    device.poll(wgpu::Maintain::Wait);
    mapping.block_on()?;

    let padded_data = buffer_slice.get_mapped_range();
    let mut pixels: Vec<u8> = vec![0; unpadded_bytes_per_row * height as usize];
    for (padded, pixels) in padded_data.
        chunks_exact(padded_bytes_per_row).
        zip(pixels.chunks_exact_mut(unpadded_bytes_per_row))
    {
        pixels.copy_from_slice(&padded[..unpadded_bytes_per_row]);
    }

    if let Some(output_image) = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, &pixels[..]){
        output_image.save("sushi-grayscale.png")?;
    }

    Ok(())
}

fn compute_work_group_count(
    (width, height): (u32, u32),
    (workgroup_width, workgroup_height): (u32, u32),
) -> (u32, u32) {
    let x = (width + workgroup_width - 1) / workgroup_width;
    let y = (height + workgroup_height -1 ) / workgroup_height;

    return (x, y);
}

fn padded_bytes_per_row(width: u32) -> usize {
    let bytes_per_row = width as usize * 4;
    let padding = (256 - bytes_per_row % 256) % 256;
    bytes_per_row + padding
}
