/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct PushData {
    bool vertical;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;

[[vk::binding(0, 0)]]
RWTexture2D<float4> Blurring;

const float WEIGHTS[] = {
    0.227027,
    0.1945946,
    0.1216216,
    0.054054,
    0.016216,
};

void ComputeMain(uint3 idx : SV_DispatchThreadID)
{
    uint width;
    uint height;
    Blurring.GetDimensions(width, height);

    uint2 dims = uint2(width - 1, height - 1);

    uint2 coord = idx.xy;

    float3 blurred_texel = Blurring[coord].rgb * WEIGHTS[0];

    if (push_data.vertical) {

        for (int i = 1; i < 5; i++) {
            uint2 sample_coord = coord + uint2(i, 0);
            sample_coord = clamp(sample_coord, uint2(0, 0), dims);
            blurred_texel += Blurring[sample_coord] * WEIGHTS[i];

            sample_coord = coord - uint2(i, 0);
            sample_coord = clamp(sample_coord, uint2(0, 0), dims);
            blurred_texel += Blurring[sample_coord] * WEIGHTS[i];
        }

    } else {
        for (int i = 1; i < 5; i++) {
            uint2 sample_coord = coord + uint2(0, i);
            sample_coord = clamp(sample_coord, uint2(0, 0), dims);
            blurred_texel += Blurring[sample_coord].rgb * WEIGHTS[i];

            sample_coord = coord - uint2(0, i);
            sample_coord = clamp(sample_coord, uint2(0, 0), dims);
            blurred_texel += Blurring[sample_coord].rgb * WEIGHTS[i];
        }

    }

    Blurring[idx.xy] = float4(blurred_texel, 1.0);
}
