Shader "Hidden/ReScaleFrame/Copy"
{
    SubShader
    {
        Tags { "RenderPipeline" = "UniversalPipeline" }
        ZWrite Off ZTest Always Cull Off
        HLSLINCLUDE
        #pragma target 4.5
        #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Core.hlsl"
        #include "Packages/com.unity.render-pipelines.core/Runtime/Utilities/Blit.hlsl"
        float4 Copy(Varyings input) : SV_Target { return SAMPLE_TEXTURE2D_X(_BlitTexture, sampler_LinearClamp, input.texcoord); }
        float4 Depth(Varyings input) : SV_Target { return SAMPLE_TEXTURE2D_X(_BlitTexture, sampler_PointClamp, input.texcoord).rrrr; }
        float4 Display(Varyings input) : SV_Target {
            float4 color = SAMPLE_TEXTURE2D_X(_BlitTexture, sampler_LinearClamp, input.texcoord);
            #ifndef UNITY_COLORSPACE_GAMMA
            color.rgb = LinearToSRGB(color.rgb);
            #endif
            return color;
        }
        ENDHLSL
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Copy
        ENDHLSL }
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Depth
        ENDHLSL }
        Pass { HLSLPROGRAM
            #pragma vertex Vert
            #pragma fragment Display
        ENDHLSL }
    }
}
