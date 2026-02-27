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
    @location(1) model_normal: vec3<f32>,
}

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VertexOutput {
    var output: VertexOutput;

    // Transform vertex position by model-view and projection matrices
    let world_pos = uniforms.model_view * vec4<f32>(position, 1.0);
    output.position = uniforms.projection * world_pos;
    output.world_position = world_pos.xyz;
    
    // For a unit sphere centered at origin, the position IS the normal
    output.model_normal = position;

    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // For contours: discard fragments that shouldn't be visible
    
    // 1. Discard fragments where the vertex is below the surface (radius < 1.0)
    //    This culls the interior segments that connect countries through the globe
    let model_radius = length(input.model_normal);
    if (model_radius < 0.995) {
        discard;
    }
    
    // 2. Discard backfacing fragments to avoid blending artifacts with semi-transparent overlays
    // Transform model-space normal to view space using the model-view matrix
    // (rotation part only, since normals don't translate)
    let view_normal = normalize(
        uniforms.model_view[0].xyz * input.model_normal.x +
        uniforms.model_view[1].xyz * input.model_normal.y +
        uniforms.model_view[2].xyz * input.model_normal.z
    );
    
    // Direction from camera (at origin in view space) to the point
    let view_dir = normalize(input.world_position);
    
    // If normal and view direction point in similar directions, surface is facing away
    // Discard backfacing fragments (dot > 0 means facing away)
    if (dot(view_normal, view_dir) > 0.0) {
        discard;
    }
    
    // Output the uniform color directly
    return uniforms.color;
}
