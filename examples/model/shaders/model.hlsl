/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

struct PushData {
    column_major float4x4 mvp;
    column_major float4x4 mv;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;


struct AssemblerOut {
    [[vk::location(0)]]
    float3 position;

    [[vk::location(1)]]
    float3 normal;
};

struct VertexOut {
    float4 position : SV_Position;
    float3 normal;
};

struct FragmentOut {
    [[vk::location(0)]]
    float4 color;
};


VertexOut VertexMain(AssemblerOut input)
{
    VertexOut output;

    float4 position = float4(input.position, 1.0);

    position = mul(push_data.mvp, position);

    output.position = position;
    output.normal = input.normal;

    return output;
}

FragmentOut FragmentMain(VertexOut input)
{
    float3 light_dir = normalize(float3(0.4, 0.0, 1.0));

    FragmentOut output;

    // nice blue-ish color
    output.color = float4(0.75, 0.9, 0.9, 1.0);

    // convert normal to world space
    float3 normal = normalize(mul(push_data.mv, float4(input.normal, 1.0)).xyz);

    output.color.rgb *= dot(light_dir, normal).xxx;

    return output;
}