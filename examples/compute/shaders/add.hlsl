/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

[[vk::binding(0, 0)]]
RWStructuredBuffer<float> output;

[[vk::binding(0, 1)]]
StructuredBuffer<float> data;

struct Input {
    uint idx : SV_DispatchThreadID;
};

void ComputeMain(Input input)
{
    output[input.idx] = data[input.idx] * 2.0;
}