package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.FloatBuffer;
import java.nio.IntBuffer;
import java.util.ArrayList;
import java.util.List;
import java.util.Arrays;
import java.util.HashMap;
import java.util.Map;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryStack;

/** Replays the real bound shaders and uniforms; no second color pass or CPU vertex reconstruction. */
final class MotionProgram implements AutoCloseable {
    final int source,program,storageBinding;
    final int[] textureUnits=new int[3];
    private final List<Uniform> uniforms=new ArrayList<>();
    private final Map<String,Integer> locations=new HashMap<>();
    private final boolean[] usedTextureUnits=new boolean[GL11.glGetInteger(GL20.GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS)];
    MotionProgram(int source) {
        this.source=source;storageBinding=freeStorageBinding(source);
        int count=GL20.glGetProgrami(source,GL20.GL_ATTACHED_SHADERS);
        int[] attached=new int[count];GL20.glGetAttachedShaders(source,(int[])null,attached);
        String vertex=null,fragment=null;
        for(int shader:attached) {
            int type=GL20.glGetShaderi(shader,GL20.GL_SHADER_TYPE);
            if(type==GL20.GL_VERTEX_SHADER)vertex=GL20.glGetShaderSource(shader);
            else if(type==GL20.GL_FRAGMENT_SHADER)fragment=GL20.glGetShaderSource(shader);
            else throw new IllegalArgumentException("Flywheel motion requires a vertex/fragment pipeline; stage "+type+" was present");
        }
        if(vertex==null||fragment==null)throw new IllegalArgumentException("Flywheel shader sources unavailable");
        int vs=compile(GL20.GL_VERTEX_SHADER,FlywheelShaders.vertex(vertex,storageBinding)),fs=0,linked=0;
        try {
            fs=compile(GL20.GL_FRAGMENT_SHADER,FlywheelShaders.fragment(fragment,
                fragment.contains("_clrwl_shader_main")?outputAlpha(source):"flw_fragColor.a"));
            linked=GL20.glCreateProgram();GL20.glAttachShader(linked,vs);GL20.glAttachShader(linked,fs);
            try(var stack=MemoryStack.stackPush()) {
                var size=stack.mallocInt(1);var type=stack.mallocInt(1);
                for(int i=0;i<GL20.glGetProgrami(source,GL20.GL_ACTIVE_ATTRIBUTES);i++) {
                    String name=GL20.glGetActiveAttrib(source,i,size,type);
                    int location=GL20.glGetAttribLocation(source,name);
                    if(location>=0&&!name.startsWith("gl_"))GL20.glBindAttribLocation(linked,location,name);
                }
            }
            GL20.glLinkProgram(linked);
            if(GL20.glGetProgrami(linked,GL20.GL_LINK_STATUS)==0)throw new IllegalStateException(GL20.glGetProgramInfoLog(linked));
            program=linked;
        } catch(RuntimeException failure) {if(linked!=0)GL20.glDeleteProgram(linked);throw failure;}
        finally {GL20.glDeleteShader(vs);if(fs!=0)GL20.glDeleteShader(fs);}
        for(int block=0;block<GL20.glGetProgrami(source,GL31.GL_ACTIVE_UNIFORM_BLOCKS);block++) {
            String name=GL31.glGetActiveUniformBlockName(source,block);
            int target=GL31.glGetUniformBlockIndex(program,name);
            if(target!=GL31.GL_INVALID_INDEX)GL31.glUniformBlockBinding(program,target,GL31.glGetActiveUniformBlocki(source,block,GL31.GL_UNIFORM_BLOCK_BINDING));
        }
        for(int block=0;block<GL43.glGetProgramInterfacei(source,GL43.GL_SHADER_STORAGE_BLOCK,GL43.GL_ACTIVE_RESOURCES);block++) {
            String name=GL43.glGetProgramResourceName(source,GL43.GL_SHADER_STORAGE_BLOCK,block);
            int target=GL43.glGetProgramResourceIndex(program,GL43.GL_SHADER_STORAGE_BLOCK,name);
            if(target!=GL31.GL_INVALID_INDEX) {
                int[] binding=new int[1];
                GL43.glGetProgramResourceiv(source,GL43.GL_SHADER_STORAGE_BLOCK,block,new int[]{GL43.GL_BUFFER_BINDING},null,binding);
                GL43.glShaderStorageBlockBinding(program,target,binding[0]);
            }
        }
        try(var stack=MemoryStack.stackPush()) {
            var size=stack.mallocInt(1);var type=stack.mallocInt(1);
            for(int i=0;i<GL20.glGetProgrami(source,GL20.GL_ACTIVE_UNIFORMS);i++) {
                String name=GL20.glGetActiveUniform(source,i,size,type);
                for(int element=0;element<size.get(0);element++) {
                    String item=name;
                    if(size.get(0)>1) {
                        int start=name.lastIndexOf("[0]");
                        if(start<0)throw new IllegalArgumentException("Unrecognized uniform array: "+name);
                        item=name.substring(0,start)+"["+element+"]"+name.substring(start+3);
                    }
                    int from=GL20.glGetUniformLocation(source,item),to=GL20.glGetUniformLocation(program,item);
                    if(from>=0&&to>=0)uniforms.add(new Uniform(from,to,type.get(0)));
                }
            }
        }
    }
    void bind() {
        GL20.glUseProgram(program);
        boolean[] used=usedTextureUnits;Arrays.fill(used,false);
        for(var uniform:uniforms) {uniform.copy(source);if(uniform.sampler) {int unit=uniform.integers.get(0);if(unit>=0&&unit<used.length)used[unit]=true;}}
        int candidate=used.length-1;
        for(int i=0;i<3;i++) {
            while(candidate>=0&&used[candidate])candidate--;
            if(candidate<0)throw new IllegalStateException("No free texture unit for Flywheel motion history");
            textureUnits[i]=candidate--;
        }
        integer("_rfc_previousVertices",textureUnits[0]);integer("_rfc_instanceMap",textureUnits[1]);integer("_rfc_drawMeshes",textureUnits[2]);
    }
    private int location(String name) {return locations.computeIfAbsent(name,key->GL20.glGetUniformLocation(program,key));}
    void integer(String name,int value) {GL20.glUniform1i(location(name),value);}
    void unsigned(String name,int value) {GL30.glUniform1ui(location(name),value);}
    void vector(String name,float x,float y) {GL20.glUniform2f(location(name),x,y);}
    private static int compile(int type,String source) {
        int shader=GL20.glCreateShader(type);GL20.glShaderSource(shader,source);GL20.glCompileShader(shader);
        if(GL20.glGetShaderi(shader,GL20.GL_COMPILE_STATUS)==0) {
            String message=GL20.glGetShaderInfoLog(shader);GL20.glDeleteShader(shader);
            throw new IllegalStateException("Flywheel motion shader: "+message);
        }
        return shader;
    }
    private static int freeStorageBinding(int source) {
        boolean[] used=new boolean[GL11.glGetInteger(GL43.GL_MAX_SHADER_STORAGE_BUFFER_BINDINGS)];
        int[] binding=new int[1];
        for(int i=0;i<GL43.glGetProgramInterfacei(source,GL43.GL_SHADER_STORAGE_BLOCK,GL43.GL_ACTIVE_RESOURCES);i++) {
            GL43.glGetProgramResourceiv(source,GL43.GL_SHADER_STORAGE_BLOCK,i,new int[]{GL43.GL_BUFFER_BINDING},null,binding);
            used[binding[0]]=true;
        }
        for(int i=used.length-1;i>=0;i--)if(!used[i])return i;
        throw new IllegalStateException("No free shader-storage binding for Flywheel motion history");
    }
    private static String outputAlpha(int source) {
        int[] properties={GL43.GL_LOCATION,GL43.GL_TYPE},values=new int[2];
        for(int i=0;i<GL43.glGetProgramInterfacei(source,GL43.GL_PROGRAM_OUTPUT,GL43.GL_ACTIVE_RESOURCES);i++) {
            GL43.glGetProgramResourceiv(source,GL43.GL_PROGRAM_OUTPUT,i,properties,null,values);
            if(values[0]==0&&values[1]==GL20.GL_FLOAT_VEC4)return GL43.glGetProgramResourceName(source,GL43.GL_PROGRAM_OUTPUT,i)+".a";
        }
        return "1.0";
    }
    private static final class Uniform {
        final int from,to,type,components;
        final boolean sampler;
        final FloatBuffer floats;
        final IntBuffer integers;
        Uniform(int from,int to,int type) {
            this.from=from;this.to=to;this.type=type;
            components=switch(type) {
                case GL20.GL_FLOAT_VEC2,GL20.GL_INT_VEC2,GL20.GL_BOOL_VEC2,GL30.GL_UNSIGNED_INT_VEC2->2;
                case GL20.GL_FLOAT_VEC3,GL20.GL_INT_VEC3,GL20.GL_BOOL_VEC3,GL30.GL_UNSIGNED_INT_VEC3->3;
                case GL20.GL_FLOAT_VEC4,GL20.GL_INT_VEC4,GL20.GL_BOOL_VEC4,GL30.GL_UNSIGNED_INT_VEC4,GL20.GL_FLOAT_MAT2->4;
                case GL21.GL_FLOAT_MAT2x3,GL21.GL_FLOAT_MAT3x2->6;
                case GL21.GL_FLOAT_MAT2x4,GL21.GL_FLOAT_MAT4x2->8;
                case GL20.GL_FLOAT_MAT3->9;
                case GL21.GL_FLOAT_MAT3x4,GL21.GL_FLOAT_MAT4x3->12;
                case GL20.GL_FLOAT_MAT4->16;
                default->1;
            };
            floats=BufferUtils.createFloatBuffer(components);integers=BufferUtils.createIntBuffer(components);
            sampler=type>=GL20.GL_SAMPLER_1D&&type<=GL31.GL_SAMPLER_2D_RECT_SHADOW ||
                type>=GL30.GL_SAMPLER_1D_ARRAY&&type<=GL30.GL_SAMPLER_CUBE_SHADOW ||
                type>=GL30.GL_INT_SAMPLER_1D&&type<=GL31.GL_UNSIGNED_INT_SAMPLER_BUFFER ||
                type>=GL32.GL_SAMPLER_2D_MULTISAMPLE&&type<=GL32.GL_UNSIGNED_INT_SAMPLER_2D_MULTISAMPLE_ARRAY ||
                type>=GL40.GL_SAMPLER_CUBE_MAP_ARRAY&&type<=GL40.GL_UNSIGNED_INT_SAMPLER_CUBE_MAP_ARRAY;
        }
        void copy(int source) {
            switch(type) {
                case GL11.GL_FLOAT,GL20.GL_FLOAT_VEC2,GL20.GL_FLOAT_VEC3,GL20.GL_FLOAT_VEC4,
                     GL20.GL_FLOAT_MAT2,GL20.GL_FLOAT_MAT3,GL20.GL_FLOAT_MAT4,
                     GL21.GL_FLOAT_MAT2x3,GL21.GL_FLOAT_MAT2x4,GL21.GL_FLOAT_MAT3x2,
                     GL21.GL_FLOAT_MAT3x4,GL21.GL_FLOAT_MAT4x2,GL21.GL_FLOAT_MAT4x3 -> {
                    GL20.glGetUniformfv(source,from,floats);
                    switch(type) {
                        case GL11.GL_FLOAT->GL20.glUniform1fv(to,floats);
                        case GL20.GL_FLOAT_VEC2->GL20.glUniform2fv(to,floats);
                        case GL20.GL_FLOAT_VEC3->GL20.glUniform3fv(to,floats);
                        case GL20.GL_FLOAT_VEC4->GL20.glUniform4fv(to,floats);
                        case GL20.GL_FLOAT_MAT2->GL20.glUniformMatrix2fv(to,false,floats);
                        case GL20.GL_FLOAT_MAT3->GL20.glUniformMatrix3fv(to,false,floats);
                        case GL20.GL_FLOAT_MAT4->GL20.glUniformMatrix4fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT2x3->GL21.glUniformMatrix2x3fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT2x4->GL21.glUniformMatrix2x4fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT3x2->GL21.glUniformMatrix3x2fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT3x4->GL21.glUniformMatrix3x4fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT4x2->GL21.glUniformMatrix4x2fv(to,false,floats);
                        case GL21.GL_FLOAT_MAT4x3->GL21.glUniformMatrix4x3fv(to,false,floats);
                    }
                }
                case GL11.GL_UNSIGNED_INT,GL30.GL_UNSIGNED_INT_VEC2,GL30.GL_UNSIGNED_INT_VEC3,GL30.GL_UNSIGNED_INT_VEC4 -> {
                    GL30.glGetUniformuiv(source,from,integers);
                    switch(components) {case 1->GL30.glUniform1uiv(to,integers);case 2->GL30.glUniform2uiv(to,integers);case 3->GL30.glUniform3uiv(to,integers);case 4->GL30.glUniform4uiv(to,integers);}
                }
                default -> {
                    GL20.glGetUniformiv(source,from,integers);
                    switch(components) {case 1->GL20.glUniform1iv(to,integers);case 2->GL20.glUniform2iv(to,integers);case 3->GL20.glUniform3iv(to,integers);case 4->GL20.glUniform4iv(to,integers);}
                }
            }
        }
    }
    @Override public void close() {GL20.glDeleteProgram(program);}
}
