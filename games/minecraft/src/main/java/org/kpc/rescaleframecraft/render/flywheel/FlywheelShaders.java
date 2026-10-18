package org.kpc.rescaleframecraft.render.flywheel;

import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Motion-only shader variants. The normal Flywheel/Iris color programs remain untouched. */
public final class FlywheelShaders {
    private FlywheelShaders() {}
    public static String vertex(String source,int storageBinding) {
        return vertex(source,storageBinding,false,false,"flw_vertexTexCoord","flw_vertexColor");
    }
    static String immediateVertex(String source,int storageBinding,String uv,String color) {
        return vertex(source,storageBinding,true,false,uv,color);
    }
    static String terrainVertex(String source,int storageBinding) {
        return vertex(source,storageBinding,true,true,"_vert_tex_diffuse_coord","_vert_color");
    }
    private static String vertex(String source,int storageBinding,boolean immediate,boolean terrain,String uv,String color) {
        boolean indirect=!immediate&&source.contains("uint _flw_instances[]");
        if(!immediate) {
        int[] unpack=function(source,"_flw_unpackInstance");
        String original=identifier(source.substring(unpack[0],unpack[1]),"_flw_unpackInstance","_rfc_unpackInstance");
        String type=indirect?"uint":"int";
        String wrapper="FlwInstance _flw_unpackInstance("+type+" index) { _rfc_instanceIndex=uint(index); return _rfc_unpackInstance(index); }\n";
        source=source.substring(0,unpack[0])+original+"\n"+wrapper+source.substring(unpack[1]);
        }
        int[] main=function(source,"main");
        String original=identifier(source.substring(main[0],main[1]),"main","_rfc_originalMain");
        if(indirect) {
            var draw=Pattern.compile("(\\w*MeshDrawCommand\\s+draw\\s*=\\s*_flw_drawCommands\\[drawIndex]\\s*;)").matcher(original);
            if(!draw.find())throw new IllegalArgumentException("Flywheel indirect draw selection changed");
            original=draw.replaceFirst("$1\n_rfc_meshOffset=_rfc_meshOverride>=0 ? uint(_rfc_meshOverride) : texelFetch(_rfc_drawMeshes,int(drawIndex)).r;");
        }
        source=source.substring(0,main[0])+original+"\n"+"""
            void main() {
                VERTEX_SETUP
                _rfc_originalMain();
                POST_VERTEX_SETUP
                _rfc_actorBatch=0u;
                if(_rfc_actorVertices) _rfc_instanceIndex=uint(gl_VertexID);
                _rfc_currentUv=CURRENT_UV;
                _rfc_currentColor=CURRENT_COLOR;
                _rfc_historyValid=0.0;
                _rfc_previousClip=gl_Position;
                _rfc_previousUv=_rfc_currentUv;
                _rfc_previousColor=_rfc_currentColor;
                if (_rfc_instanceIndex>=uint(textureSize(_rfc_instanceMap))) return;
                uvec2 addresses=texelFetch(_rfc_instanceMap,int(_rfc_instanceIndex)).rg;
                if(_rfc_actorVertices) {
                    if(addresses.y==0xffffffffu)return;
                    _rfc_actorBatch=texelFetch(_rfc_drawMeshes,gl_VertexID).r;
                }
                uint vertex=_rfc_actorVertices?0u:_rfc_meshOffset+VERTEX_ID;
                if (addresses.x!=0u) {
                    int previous=int(addresses.x-1u+vertex)*2;
                    uvec4 state=previous>=0 && previous+1<textureSize(_rfc_previousVertices)
                        ? texelFetch(_rfc_previousVertices,previous+1) : uvec4(0);
                    if (state.w==_rfc_previousFrame && _rfc_previousFrame!=0u) {
                        _rfc_previousClip=uintBitsToFloat(texelFetch(_rfc_previousVertices,previous));
                        _rfc_previousUv=uintBitsToFloat(state.xy);
                        _rfc_previousColor=unpackUnorm4x8(state.z);
                        _rfc_historyValid=1.0;
                    }
                }
                uint current=(addresses.y+vertex)*8u;
                if (current+7u>=uint(_rfc_vertices.length())) return;
                vec4 unjittered=gl_Position;
                unjittered.xy-=_rfc_currentJitter*2.0/_rfc_renderSize*unjittered.w;
                uvec4 clip=floatBitsToUint(unjittered);
                atomicExchange(_rfc_vertices[current],clip.x);
                atomicExchange(_rfc_vertices[current+1u],clip.y);
                atomicExchange(_rfc_vertices[current+2u],clip.z);
                atomicExchange(_rfc_vertices[current+3u],clip.w);
                atomicExchange(_rfc_vertices[current+4u],floatBitsToUint(_rfc_currentUv.x));
                atomicExchange(_rfc_vertices[current+5u],floatBitsToUint(_rfc_currentUv.y));
                atomicExchange(_rfc_vertices[current+6u],packUnorm4x8(_rfc_currentColor));
                atomicExchange(_rfc_vertices[current+7u],_rfc_frame);
            }
            """.replace("POST_VERTEX_SETUP",terrain?"_rfc_instanceIndex=_draw_id;":"")
                .replace("VERTEX_SETUP",immediate?"_rfc_instanceIndex=0u;_rfc_meshOffset=0u;":indirect?"":"_rfc_meshOffset=_rfc_instancedMeshOffset;")
                .replace("CURRENT_UV",uv).replace("CURRENT_COLOR",color)
                .replace("VERTEX_ID",terrain?"(uint(gl_VertexID)-texelFetch(_rfc_drawMeshes,int(_draw_id)).r)":immediate?"uint(gl_VertexID)":"flw_vertexId")+source.substring(main[1]);
        String header="""
            #extension GL_ARB_shader_storage_buffer_object : require
            #extension GL_ARB_shading_language_packing : require
            uniform usamplerBuffer _rfc_previousVertices;
            uniform usamplerBuffer _rfc_instanceMap;
            uniform usamplerBuffer _rfc_drawMeshes;
            uniform uint _rfc_frame;
            uniform uint _rfc_previousFrame;
            uniform int _rfc_meshOverride;
            uniform uint _rfc_instancedMeshOffset;
            uniform vec2 _rfc_currentJitter;
            uniform vec2 _rfc_renderSize;
            uniform bool _rfc_actorVertices;
            uint _rfc_instanceIndex;
            uint _rfc_meshOffset;
            out vec4 _rfc_previousClip;
            out vec2 _rfc_previousUv;
            out vec4 _rfc_previousColor;
            out vec2 _rfc_currentUv;
            out vec4 _rfc_currentColor;
            out float _rfc_historyValid;
            flat out uint _rfc_actorBatch;
            layout(std430,binding=STORAGE_BINDING) coherent buffer RfcVertexHistory { uint _rfc_vertices[]; };
            """.replace("STORAGE_BINDING",Integer.toString(storageBinding));
        return insertHeader(source,header);
    }
    public static String fragment(String source) {return fragment(source,"flw_fragColor.a");}
    public static String fragment(String source,String alphaExpression) {
        // Preserve the complete original material and shader-pack entry point,
        // including discard, while redirecting its MRT outputs into private globals.
        var outputs=Pattern.compile("(?m)^\\s*(?:layout\\s*\\([^\\n]*\\)\\s*)?(?:(?:flat|smooth|noperspective|centroid|sample|invariant)\\s+)*out\\s+([^;]+);").matcher(source);
        var result=new StringBuilder();
        while(outputs.find())if(topLevel(source,outputs.start())&&!outputs.group(1).matches(".*\\bgl_FragDepth\\b.*"))
            outputs.appendReplacement(result,Matcher.quoteReplacement(outputs.group(1)+";"));
        outputs.appendTail(result);source=result.toString();
        source=identifier(source,"gl_FragData","_rfc_originalFragData");
        source=identifier(source,"gl_FragColor","_rfc_originalFragColor");
        boolean writesDepth=Pattern.compile("\\bgl_FragDepth\\s*=").matcher(source).find();
        int[] main=function(source,"main");
        String original=identifier(source.substring(main[0],main[1]),"main","_rfc_originalMain");
        source=source.substring(0,main[0])+original+"\n"+"""
            void main() {
                vec2 oldPixel=(_rfc_previousClip.xy/max(_rfc_previousClip.w,1e-6)*0.5+0.5)*_rfc_renderSize;
                float oldInverse=_rfc_useDepthDecode ? dot(_rfc_depthDecode,vec2(_rfc_previousClip.z/max(_rfc_previousClip.w,1e-6),1.0)) : 1.0/max(_rfc_previousClip.w,1e-6);
                vec2 dx=dFdx(oldPixel),dy=dFdy(oldPixel);
                vec2 dz=vec2(dFdx(oldInverse),dFdy(oldInverse));
                float determinant=dx.x*dy.y-dx.y*dy.x;
                bool invertible=determinant>1e-8;
                _rfc_plane=invertible?vec2(dz.x*dy.y-dz.y*dx.y,dx.x*dz.y-dy.x*dz.x)/determinant:vec2(0.0);
                if(any(isnan(_rfc_plane))||any(isinf(_rfc_plane))||any(greaterThan(abs(_rfc_plane),vec2(65504.0)))) {_rfc_plane=vec2(0.0);invertible=false;}
                _rfc_originalMain();
                if(_rfc_actorVertices&&_rfc_actorBatch==0u)discard;
                float alpha=ALPHA_EXPRESSION;
                if (alpha<0.01) discard;
                float windowDepth=WINDOW_DEPTH;
                float depth=_rfc_reversedDepth ? 1.0-windowDepth : windowDepth;
                float reactive=max(1.0-_rfc_historyValid,1.0-alpha);
                if(!invertible)reactive=1.0;
                if (any(greaterThan(abs(_rfc_previousUv-_rfc_currentUv),vec2(0.000001))) ||
                    any(greaterThan(abs(_rfc_previousColor-_rfc_currentColor),vec4(0.004)))) reactive=1.0;
                vec2 delta=vec2(0.0);
                if (_rfc_historyValid>0.99999 && _rfc_previousClip.w>0.000001) {
                    vec2 oldPixel=(_rfc_previousClip.xy/_rfc_previousClip.w*0.5+0.5)*_rfc_renderSize;
                    delta=oldPixel-(gl_FragCoord.xy-_rfc_currentJitter);
                } else reactive=1.0;
                _rfc_motion=vec4(delta,reactive,_rfc_previousClip.w>0.000001 ? max(oldInverse,0.0) : 0.0);
                _rfc_depth=depth;
                _rfc_actorId=_rfc_actorBatch;
            }
            """.replace("WINDOW_DEPTH",writesDepth?"gl_FragDepth":"gl_FragCoord.z")
                .replace("ALPHA_EXPRESSION",alphaExpression.replace("gl_FragData","_rfc_originalFragData").replace("gl_FragColor","_rfc_originalFragColor"))+source.substring(main[1]);
        return insertHeader(source,"""
            vec4 _rfc_originalFragData[8];
            vec4 _rfc_originalFragColor;
            uniform vec2 _rfc_renderSize;
            uniform vec2 _rfc_currentJitter;
            uniform bool _rfc_reversedDepth;
            uniform bool _rfc_useDepthDecode;
            uniform bool _rfc_actorVertices;
            uniform vec2 _rfc_depthDecode;
            in vec4 _rfc_previousClip;
            in vec2 _rfc_previousUv;
            in vec4 _rfc_previousColor;
            in vec2 _rfc_currentUv;
            in vec4 _rfc_currentColor;
            in float _rfc_historyValid;
            flat in uint _rfc_actorBatch;
            layout(location=0) out vec4 _rfc_motion;
            layout(location=1) out float _rfc_depth;
            layout(location=2) out vec2 _rfc_plane;
            layout(location=3) out uint _rfc_actorId;
            """);
    }
    private static String identifier(String source,String from,String to) {return source.replaceAll("\\b"+Pattern.quote(from)+"\\b",to);}
    private static String insertHeader(String source,String header) {
        int end=0;var lines=Pattern.compile("(?m)^.*(?:\\n|$)").matcher(source);
        while(lines.find()) {String line=lines.group().strip();if(line.isEmpty()||line.startsWith("#")||line.startsWith("//"))end=lines.end();else break;}
        return source.substring(0,end)+header+source.substring(end);
    }
    private static int[] function(String source,String name) {
        var match=Pattern.compile("\\b(?:void|FlwInstance)\\s+"+Pattern.quote(name)+"\\s*\\([^)]*\\)\\s*\\{").matcher(source);
        if(!match.find())throw new IllegalArgumentException("Flywheel shader function changed: "+name);
        int depth=1;int i=match.end();boolean line=false,comment=false;
        for(;i<source.length()&&depth!=0;i++) {
            char c=source.charAt(i),next=i+1<source.length()?source.charAt(i+1):0;
            if(line) {if(c=='\n')line=false;continue;}if(comment) {if(c=='*'&&next=='/') {comment=false;i++;}continue;}
            if(c=='/'&&next=='/') {line=true;i++;continue;}if(c=='/'&&next=='*') {comment=true;i++;continue;}
            if(c=='{')depth++;else if(c=='}')depth--;
        }
        if(depth!=0)throw new IllegalArgumentException("Unterminated Flywheel shader function: "+name);
        return new int[]{match.start(),i};
    }
    private static boolean topLevel(String source,int until) {
        int braces=0,parentheses=0;boolean line=false,comment=false;
        for(int i=0;i<until;i++) {
            char c=source.charAt(i),next=i+1<until?source.charAt(i+1):0;
            if(line) {if(c=='\n')line=false;continue;}if(comment) {if(c=='*'&&next=='/') {comment=false;i++;}continue;}
            if(c=='/'&&next=='/') {line=true;i++;continue;}if(c=='/'&&next=='*') {comment=true;i++;continue;}
            if(c=='{')braces++;else if(c=='}')braces--;if(c=='(')parentheses++;else if(c==')')parentheses--;
        }
        return braces==0&&parentheses==0&&!line&&!comment;
    }
}
