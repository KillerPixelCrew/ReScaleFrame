Shader "Hidden/ReScaleFrame/FSR1"
{
    SubShader
    {
        Tags { "RenderPipeline" = "UniversalPipeline" }
        ZWrite Off ZTest Always Cull Off
        HLSLINCLUDE
        #pragma target 4.5
        #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Core.hlsl"
        #include "Packages/com.unity.render-pipelines.core/Runtime/Utilities/Blit.hlsl"
        #define FSR_INPUT_TEXTURE _BlitTexture
        #define FSR_INPUT_SAMPLER sampler_LinearClamp
        #define FSR_ENABLE_ALPHA 1
        #include "Packages/com.unity.render-pipelines.core/Runtime/PostProcessing/Shaders/FSRCommon.hlsl"
        float4 Prepare(Varyings input) : SV_Target {
            float4 color = SAMPLE_TEXTURE2D_X(_BlitTexture, sampler_LinearClamp, input.texcoord);
            color.rgb = LinearToGamma20(FastTonemap(max(0, color.rgb)));
            return color;
        }
        float4 Upscale(Varyings input) : SV_Target {
            float3 color = Gamma20ToLinear(ApplyEASU(uint2(input.positionCS.xy)));
            return float4(color, SAMPLE_TEXTURE2D_X(_BlitTexture, sampler_LinearClamp, input.texcoord).a);
        }
        float4 Sharpen(Varyings input) : SV_Target {
            float4 color = ApplyRCAS(uint2(input.positionCS.xy));
            color.rgb = FastTonemapInvert(min(color.rgb, 0.9999));
            return color;
        }
        ENDHLSL
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Prepare
        ENDHLSL }
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Upscale
        ENDHLSL }
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Sharpen
        ENDHLSL }
    }
}
