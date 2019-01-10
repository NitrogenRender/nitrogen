/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

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
    float color_red;
    [[vk::location(1)]]
    float color_green;
    [[vk::location(2)]]
    float color_blue;
};

[[vk::binding(0, 0)]]
Texture2D t;
[[vk::binding(1, 0)]]
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

    float4 combined = t.Sample(s, input.uv);

    output.color_red = combined.r;
    output.color_green = combined.g;
    output.color_blue = combined.b;

    return output;
}
