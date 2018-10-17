#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) out vec2 v_uv;

out gl_PerVertex {
    vec4 gl_Position;
};

vec2 positions[6] = vec2[](
    vec2(-1.0, -1.0),
    vec2(1.0, -1.0),
    vec2(-1.0, 1.0),

    vec2(-1.0, 1.0),
    vec2(1.0, -1.0),
    vec2(1.0, 1.0)
);

vec2 uvs[6] = vec2[](
    vec2(0.0, 0.0),
    vec2(1.0, 0.0),
    vec2(0.0, 1.0),

    vec2(0.0, 1.0),
    vec2(1.0, 0.0),
    vec2(1.0, 1.0)
);



void main() {
    vec2 pos = positions[gl_VertexIndex];
    vec2 uv = uvs[gl_VertexIndex];
    v_uv = uv;
    gl_Position = vec4(pos, 0.0, 1.0);
}