/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */


[[vk::constant_id(0)]]
const float ADD = 0.0;


[[vk::binding(0, 0)]]
RWStructuredBuffer<float> data;

struct DispatchInput {
    uint idx : SV_DispatchThreadID;
};

void ComputeMain(DispatchInput input)
{
    data[input.idx] += ADD;
}