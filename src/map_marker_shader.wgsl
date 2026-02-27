// Screen-space marker shader for 3D globe
// Renders a circular marker with glow effect at a 3D position
// The marker maintains constant screen size regardless of perspective

struct MarkerUniforms {
    projection: mat4x4<f32>,
    model_view: mat4x4<f32>,
    marker_center: vec3<f32>,
    marker_radius: f32,
    viewport_size: vec2<f32>,
    time: f32,
    _padding: f32,
    color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: MarkerUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) view_pos: vec3<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    
    // Transform marker center to clip space
    let world_pos = uniforms.model_view * vec4<f32>(uniforms.marker_center, 1.0);
    let clip_pos = uniforms.projection * world_pos;
    
    // Convert to normalized device coordinates (NDC)
    let ndc = clip_pos.xyz / clip_pos.w;
    
    // Calculate screen-space offset for each vertex (quad corners)
    // Vertex indices: 0=bottom-left, 1=bottom-right, 2=top-right, 3=top-left
    var corner_offset = vec2<f32>(0.0, 0.0);
    var uv_coord = vec2<f32>(0.0, 0.0);
    
    switch vertex_index {
        case 0u: {
            corner_offset = vec2<f32>(-1.0, -1.0);
            uv_coord = vec2<f32>(0.0, 1.0);
        }
        case 1u: {
            corner_offset = vec2<f32>(1.0, -1.0);
            uv_coord = vec2<f32>(1.0, 1.0);
        }
        case 2u: {
            corner_offset = vec2<f32>(1.0, 1.0);
            uv_coord = vec2<f32>(1.0, 0.0);
        }
        default: { // case 3u
            corner_offset = vec2<f32>(-1.0, 1.0);
            uv_coord = vec2<f32>(0.0, 0.0);
        }
    }
    
    // Convert marker radius from pixels to NDC
    let radius_ndc = vec2<f32>(
        uniforms.marker_radius / uniforms.viewport_size.x * 2.0,
        uniforms.marker_radius / uniforms.viewport_size.y * 2.0
    );
    
    // Apply screen-space offset
    let offset_ndc = corner_offset * radius_ndc;
    let final_ndc = ndc.xy + offset_ndc;
    
    // Output position in clip space (reconstruct w component)
    output.position = vec4<f32>(final_ndc * clip_pos.w, ndc.z * clip_pos.w, clip_pos.w);
    output.uv = uv_coord;
    output.view_pos = world_pos.xyz;
    
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Check if marker is on the far side of the sphere
    // Transform the marker normal (model space) to view space
    let marker_normal_view = normalize((uniforms.model_view * vec4<f32>(uniforms.marker_center, 0.0)).xyz);
    
    // Direction from camera (at origin in view space) to marker
    let view_dir = normalize(input.view_pos);
    
    // If the normal points away from camera (same direction as view_dir), marker is on far side
    if (dot(marker_normal_view, view_dir) > 0.0) {
        discard;
    }
    
    // Calculate distance from center in pixel space
    let center = vec2<f32>(0.5, 0.5);
    let dist_uv = distance(input.uv, center);
    
    // Convert UV distance to pixel distance
    // In UV space, distance 1.0 = marker_radius pixels (from center to edge)
    let dist_pixels = dist_uv * 2.0 * uniforms.marker_radius;
    
    // Define sizes in pixels
    let solid_radius = 9.0;        // Solid circle radius in pixels
    var glow_radius = 30.0;        // Glow extends to this radius in pixels
    
    // Pulsating animation when green (connected)
    let is_green = uniforms.color.g > 0.7 && uniforms.color.r < 0.3;
    if (is_green) {
        let pulse_freq = 0.7; // 0.4 Hz = 1 pulse every 2.5 seconds
        glow_radius = glow_radius * (1.0 + sin(uniforms.time * 6.28318 * pulse_freq) * 0.15);
    }
    
    var alpha = 0.0;
    
    if (dist_pixels < solid_radius) {
        // Solid inner circle with smooth edge (no pulsing)
        alpha = smoothstep(solid_radius + 3.0, solid_radius - 3.0, dist_pixels);
    } else if (dist_pixels < glow_radius) {
        // Glow effect - exponential falloff (with pulsing)
        let glow_factor = (glow_radius - dist_pixels) / (glow_radius - solid_radius);
        alpha = pow(glow_factor, 2.0) * 0.6;
    }
    
    return vec4<f32>(uniforms.color.rgb, uniforms.color.a * alpha);
}
