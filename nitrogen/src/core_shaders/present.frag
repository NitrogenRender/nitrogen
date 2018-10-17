#version 450
#extension GL_ARB_separate_shader_objects : enable


layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 o_color;

layout(set = 0, binding = 0) uniform texture2D u_texture;
layout(set = 0, binding = 1) uniform sampler u_sampler;

void main() {
    o_color = texture(sampler2D(u_texture, u_sampler), v_uv);
    // o_color = vec4(v_uv, 1.0, 1.0);
}