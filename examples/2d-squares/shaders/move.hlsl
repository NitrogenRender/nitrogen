/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct ComputeInput {
    uint3 idx : SV_DispatchThreadID;
};


struct PushData {
    uint batch_size;
    uint total_size;
    float delta;
};



struct Instance {
    float2 position;
    float2 size;
    float4 color;
};

[[vk::binding(0, 1)]]
RWStructuredBuffer<Instance> instances;

[[vk::binding(1, 1)]]
RWStructuredBuffer<float2> velocities;


[[vk::push_constant]]
ConstantBuffer<PushData> push_data;

void ComputeMain(ComputeInput input)
{
    uint idx = input.idx.x + (push_data.batch_size * input.idx.y);

    if (idx > push_data.total_size)
        return;

    float2 position = instances[idx].position += velocities[idx] * push_data.delta;

    if (position.x < -1.0 || position.x > 1.0) {
        position.x = clamp(position.x, -1.0, 1.0);
        velocities[idx].x *= -1.0;
    }

    if (position.y < -1.0 || position.y > 1.0) {
        position.y = clamp(position.y, -1.0, 1.0);
        velocities[idx].y *= -1.0;
    }

    instances[idx].position = position;
}