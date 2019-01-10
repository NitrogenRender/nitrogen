/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct PushData {
    float x;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;


[[vk::binding(0, 0)]]
RWStructuredBuffer<float> data;

struct DispatchInput {
    uint idx : SV_DispatchThreadID;
};

void ComputeMain(DispatchInput input)
{
    data[input.idx] += push_data.x;
}