/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct AssemblyOut {
    [[vk::location(0)]]
    float2 Pos;
    [[vk::location(1)]]
    float2 Uv;
};

struct VertexOut {
    float4 PositionNdc : SV_POSITION;
    float2 Position;
    float2 Uv;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 Color;
};

VertexOut VertexMain(AssemblyOut input)
{
    VertexOut output;

    output.PositionNdc = float4(input.Pos, 0.0, 1.0);
    output.Position = input.Pos;
    output.Uv = input.Uv;

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.Color = float4(input.Uv, 1.0, 1.0);

    return output;
}