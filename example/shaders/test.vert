#version 450
#extension GL_ARB_separate_shader_objects : enable

out gl_PerVertex {
    vec4 gl_Position;
};

void VertexMain() {
    gl_Position = vec4(0.0, 0.0, 0.0, 1.0);
}



void main() {
    VertexMain();
}
