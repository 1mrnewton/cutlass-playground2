// Sample effect pass for the WGSL shader library.
//
// Effects in Cutlass are authored ONCE in WGSL and compiled by `naga` to
// Metal / Vulkan / D3D12 / GLES, so the same shader yields the same pixels on
// every platform. Each `Pass` in a `cutlass_effects::EffectGraph` references a
// shader here by id and supplies animatable params as uniforms.
//
// This is a minimal separable-blur fragment stub to anchor the pipeline; it is
// not wired to the GPU yet (the wgpu backend lands with the first render spike).

struct Params {
    // Blur radius in texels. Exposed as an animatable Track<f32> on the host.
    radius: f32,
    // 0 = horizontal pass, 1 = vertical pass.
    direction: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(src));
    let texel = 1.0 / dims;
    let step = select(vec2<f32>(0.0, texel.y), vec2<f32>(texel.x, 0.0), params.direction < 0.5);

    var color = vec4<f32>(0.0);
    var weight_sum = 0.0;
    let r = i32(clamp(params.radius, 0.0, 32.0));
    for (var i = -r; i <= r; i = i + 1) {
        let w = 1.0; // Placeholder box weights; Gaussian weights come later.
        color = color + textureSample(src, src_sampler, uv + step * f32(i)) * w;
        weight_sum = weight_sum + w;
    }
    return color / max(weight_sum, 1.0);
}
