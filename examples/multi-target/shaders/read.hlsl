/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct VertexIn {
    [[vk::location(0)]]
    float2 position;
    [[vk::location(1)]]
    float2 uv;

    uint instance_id: SV_InstanceID;
};

struct VertexOut {
    float2 uv;
    float4 position : SV_Position;

    uint idx;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

VertexOut VertexMain(VertexIn input)
{
    VertexOut output;

    uint row = input.instance_id / 2;
    uint col = input.instance_id % 2;

    float2 position = (input.position + float2(1.0, 1.0)) / 2.0;
    position.x += col * 1.0;
    position.y += row * 1.0;

    position = position - float2(1.0, 1.0);

    output.position = float4(position, 0.0, 1.0);
    output.uv = input.uv;

    output.idx = input.instance_id;

    return output;
}

[[vk::binding(0, 0)]]
Texture2D tRed;
[[vk::binding(1, 0)]]
SamplerState sRed;
[[vk::binding(2, 0)]]
Texture2D tGreen;
[[vk::binding(3, 0)]]
SamplerState sGreen;
[[vk::binding(4, 0)]]
Texture2D tBlue;
[[vk::binding(5, 0)]]
SamplerState sBlue;

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    float2 uv = input.uv;

    float4 color;
    switch (input.idx) {
    case 0:
        color = float4(tRed.Sample(sRed, uv).xxx, 1.0);
        break;
    case 1:
        color = float4(tGreen.Sample(sGreen, uv).xxx, 1.0);
        break;
    case 2:
        color = float4(tBlue.Sample(sBlue, uv).xxx, 1.0);
        break;
    case 3:
        float red = tRed.Sample(sRed, uv).x;
        float green = tGreen.Sample(sRed, uv).x;
        float blue = tBlue.Sample(sRed, uv).x;
        color = float4(red, green, blue, 1.0);
        break;
    }

    output.color = color;

    return output;
}
