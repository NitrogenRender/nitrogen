/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct PushData {
    column_major float3x3 view;
    column_major float3x3 model;
    float4 quad_color;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;

struct AssemblerOut {
    uint idx : SV_VertexID;
};

struct VertexOut {
    float4 position : SV_Position;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};

VertexOut VertexMain(AssemblerOut input)
{
    float2 positions[] = {
        float2(-1.0, -1.0),
        float2(-1.0, +1.0),
        float2(+1.0, -1.0),
        float2(+1.0, +1.0)
    };

    VertexOut output;

    float3 position = float3(positions[input.idx], 1.0);
    position = mul(push_data.model, position);
    position = mul(push_data.view, position);

    output.position = float4(position.xy, 0.0, 1.0);

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.color = push_data.quad_color;

    return output;
}