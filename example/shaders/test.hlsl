struct VertexIn {
    int vertex_id : SV_VertexID;
};

struct VertexOut {
    float4 position : SV_Position;
    float2 uv;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

VertexOut VertexMain(VertexIn input)
{
    VertexOut output;

    float2 positions[] = {
    	float2(-1.0, -1.0),
    	float2(1.0, -1.0),
    	float2(-1.0, 1.0),

    	float2(-1.0, 1.0),
    	float2(1.0, -1.0),
    	float2(1.0, 1.0)
    };

    float2 uvs[] = {
        float2(0.0, 0.0),
        float2(1.0, 0.0),
        float2(0.0, 1.0),

        float2(0.0, 1.0),
        float2(1.0, 0.0),
        float2(1.0, 1.0)
    };

    output.position = float4(positions[input.vertex_id], 0.0, 1.0);
    output.uv = uvs[input.vertex_id];

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.color = float4(input.uv, 1.0, 1.0);

    return output;
}
