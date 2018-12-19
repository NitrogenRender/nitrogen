/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct PushData {
    float2 canvas_size;
    float2 quad_pos;
    float2 quad_size;
    float quad_depth;
    float padding;
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

    float2 position = positions[input.idx];

    // get normalized position
    {
        // starting position in [-1; 1]

        // position in [0; 2]
        position = position + float2(1.0, 1.0);

        // position in [0; 1]
        position = position / 2.0;
    }

    // apply instance transform
    {
        // apply size
        position = position * push_data.quad_size;

        // apply position
        position = push_data.quad_pos + position;
    }

    // move back to NDC
    {
        // [0; 1]
        position = position / push_data.canvas_size;

        // [0; 2]
        position = position * 2.0;

        // [-1; 1]
        position = position - float2(1.0, 1.0);
    }

    output.position = float4(position, push_data.quad_depth, 1.0);

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut output;

    output.color = push_data.quad_color;

    return output;
}