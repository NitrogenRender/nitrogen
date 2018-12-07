/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct VertexIn {
    [[vk::location(0)]]
    float2 position;

    uint primitive_id: SV_InstanceID;
};

struct VertexOut {
    float4 position : SV_Position;
    float2 vert;

    uint idx;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};


struct InstanceData {
    float2 position;
    float2 size;
    float4 color;
};

[[vk::binding(0, 1)]]
cbuffer {
    InstanceData data[];
};

VertexOut VertexMain(VertexIn input)
{
    VertexOut ret;
    ret.idx = input.primitive_id;

    float2 position;
    position = input.position * data[ret.idx].size;
    position += data[ret.idx].position;

    ret.position = float4(position, 0.0, 1.0);
    ret.vert = input.position;


    return ret;
}

FragmentOut FragmentMain(VertexOut input)
{
    FragmentOut ret;

    float distance = length(input.vert);

    if (distance > 1.0) {
        discard;
    }

    ret.color = data[input.idx].color;

    return ret;
}