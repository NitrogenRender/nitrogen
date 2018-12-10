struct VertexIn {
    [[vk::location(0)]]
    float2 position;
    [[vk::location(1)]]
    float2 uv;
};

struct VertexOut {
    float2 uv;
    float4 position : SV_Position;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

VertexOut VertexMain(VertexIn input)
{
    VertexOut output;

    output.position = float4(input.position, 0.0, 1.0);
    output.uv = input.uv;

    return output;
}

[[vk::binding(0, 0)]]
Texture2D t;
[[vk::binding(1, 0)]]
SamplerState s;

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    float2 uv = input.uv;

    output.color = t.Sample(s, uv);

    return output;
}
