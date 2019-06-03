/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct VertexIn {
    [[vk::location(0)]]
    float2 Position;
    [[vk::location(1)]]
    float2 Uv;
};

struct VertexOut {
    float2 Uv;
    float4 Position : SV_Position;
    float2 PositionNormalized;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 Color;
};

VertexOut VertexMain(VertexIn input)
{
    VertexOut output;

    output.PositionNormalized = input.Position;
    output.Position = float4(input.Position, 0.0, 1.0);
    output.Uv = input.Uv;

    return output;
}

[[vk::binding(0, 0)]]
Texture2D t;
[[vk::binding(1, 0)]]
SamplerState s;

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    float2 uv = input.Uv;

    float fac = 1.0 - step(1.0, length(input.PositionNormalized));

    output.Color = t.Sample(s, uv) * fac;

    return output;
}
