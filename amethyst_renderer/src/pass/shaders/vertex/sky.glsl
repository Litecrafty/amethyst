// TODO: Needs documentation.

#version 150 core

layout (std140) uniform VertexArgs {
    uniform mat4 proj;
    uniform mat4 view;
};

in vec3 position;

out vec3 TexCoords;

void main() {
    TexCoords = position;
    mat4 v = mat4(mat3(view)); //remove translation from view matrix
    gl_Position = proj * v * vec4(position, 1.0);
}
