/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

[[vk::binding(1, 0)]]
Texture2D<float4> InputTex;
[[vk::binding(2, 0)]]
SamplerState InputSampler;

[[vk::binding(0, 0)]]
RWTexture2D<float4> Output;


void ComputeMain(uint3 idx : SV_DispatchThreadID)
{
    uint width;
    uint height;
    InputTex.GetDimensions(width, height);

    float2 uv = float2(idx.xy) / float2(width, height);

    float4 color = InputTex.Sample(InputSampler, uv);

    float brightness = dot(color.rgb, float3(0.2126, 0.7152, 0.0722));

    if (brightness < 1.0) {
        Output[idx.xy] = float4(0.0, 0.0, 0.0, 1.0);
    } else {
        Output[idx.xy] = float4(color.rgb, 1.0);
    }

}