struct VertexOut {
    float4 position: SV_Position;
    float2 uv;
};

struct VertexIn {
    int vertex_id : SV_ViewportArrayIndex;
};

VertexOut VertexMain()
{
    VertexOut output;

    output.position = float4(0.0, 0.0, 0.0, 1.0);
    output.uv = float2(0.0, 1.0);

    return output;
}

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.color = float4(input.uv, 1.0, 1.0);

    return output;
}