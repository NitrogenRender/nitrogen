[[vk::binding(0, 0)]]
RWTexture2D<float4> Combined;

[[vk::binding(1, 0)]]
Texture2D<float4> Raw;
[[vk::binding(2, 0)]]
SamplerState RawSampler;

[[vk::binding(3, 0)]]
RWTexture2D<float4> Blur;

void ComputeMain(uint3 idx : SV_DispatchThreadID)
{
    Combined[idx.xy] = float4(Raw[idx.xy].rgb + Blur[idx.xy].rgb, 1.0);
    // Combined[idx.xy] = float4(1.0, 1.0, 1.0, 1.0);
}