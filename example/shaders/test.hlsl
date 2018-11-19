struct VertexIn {
    int vertex_id : SV_VertexID;

    [[vk::location(0)]]
    float2 position;

    [[vk::location(1)]]
    float2 uv;
};

struct VertexOut {
    float4 position : SV_Position;
    float2 uv;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

[[vk::binding(0, 1)]]
Texture2D t;
[[vk::binding(1, 1)]]
SamplerState s;

VertexOut VertexMain(VertexIn input)
{
    VertexOut output;

    output.position = float4(input.position, 0.0, 1.0);
    output.uv = input.uv;

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    // output.color = float4(input.uv, 1.0, 1.0);

    output.color = t.Sample(s, input.uv);

    return output;
}
