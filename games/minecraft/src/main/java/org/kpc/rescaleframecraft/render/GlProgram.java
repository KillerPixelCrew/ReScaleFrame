package org.kpc.rescaleframecraft.render;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import org.joml.Matrix4f;
import org.lwjgl.opengl.GL20;
import org.lwjgl.system.MemoryStack;

final class GlProgram implements AutoCloseable {
    private final int program;
    private final Map<String,Integer> locations=new HashMap<>();

    GlProgram(String vertex,String fragment) {
        int vs=compile(GL20.GL_VERTEX_SHADER,read(vertex));
        int fs=compile(GL20.GL_FRAGMENT_SHADER,read(fragment));
        program=GL20.glCreateProgram();
        GL20.glAttachShader(program,vs);GL20.glAttachShader(program,fs);
        GL20.glLinkProgram(program);
        GL20.glDeleteShader(vs);GL20.glDeleteShader(fs);
        if(GL20.glGetProgrami(program,GL20.GL_LINK_STATUS)==0) {
            String error=GL20.glGetProgramInfoLog(program);GL20.glDeleteProgram(program);
            throw new IllegalStateException("Shader link failed: "+fragment+"\n"+error);
        }
    }
    private static String read(String name) {
        String path="/assets/rescaleframecraft/shaders/"+name;
        try(var input=GlProgram.class.getResourceAsStream(path)) {
            if(input==null)throw new IOException("Missing shader "+path);
            return new String(input.readAllBytes(),StandardCharsets.UTF_8);
        } catch(IOException e) { throw new IllegalStateException(e); }
    }
    private static int compile(int type,String source) {
        int shader=GL20.glCreateShader(type);
        GL20.glShaderSource(shader,source);GL20.glCompileShader(shader);
        if(GL20.glGetShaderi(shader,GL20.GL_COMPILE_STATUS)==0) {
            String error=GL20.glGetShaderInfoLog(shader);GL20.glDeleteShader(shader);
            throw new IllegalStateException("Shader compilation failed: "+error);
        }
        return shader;
    }
    void use() { GL20.glUseProgram(program); }
    int location(String name) { return locations.computeIfAbsent(name,key->GL20.glGetUniformLocation(program,key)); }
    void integer(String name,int value) { GL20.glUniform1i(location(name),value); }
    void scalar(String name,float value) { GL20.glUniform1f(location(name),value); }
    void vector2(String name,float x,float y) { GL20.glUniform2f(location(name),x,y); }
    void vector3(String name,float x,float y,float z) { GL20.glUniform3f(location(name),x,y,z); }
    void matrix(String name,Matrix4f matrix) {
        try(var stack=MemoryStack.stackPush()) { GL20.glUniformMatrix4fv(location(name),false,matrix.get(stack.mallocFloat(16))); }
    }
    @Override public void close() { GL20.glDeleteProgram(program); }
}
