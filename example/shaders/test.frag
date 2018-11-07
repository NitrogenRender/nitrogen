#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) out vec4 color;

void FragmentMain() {
    color = vec4(1.0, 1.0, 1.0, 1.0);
}

void main() {
    FragmentMain();
}