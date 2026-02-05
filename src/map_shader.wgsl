// 3D Globe shader for rendering a stylized globe map
// Renders land masses and contours with perspective projection

struct Uniforms {
    projection: mat4x4<f32>,
    model_view: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
}

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VertexOutput {
    var output: VertexOutput;

    // Transform vertex position by model-view and projection matrices
    let world_pos = uniforms.model_view * vec4<f32>(position, 1.0);
    output.position = uniforms.projection * world_pos;
    output.world_position = world_pos.xyz;

    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Output the uniform color directly
    return uniforms.color;
}
