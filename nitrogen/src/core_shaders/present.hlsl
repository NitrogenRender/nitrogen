/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

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

[[vk::binding(0, 0)]]
Texture2D tex;

[[vk::binding(1, 0)]]
SamplerState samp;


FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.color = tex.Sample(samp, input.uv);

    return output;
}