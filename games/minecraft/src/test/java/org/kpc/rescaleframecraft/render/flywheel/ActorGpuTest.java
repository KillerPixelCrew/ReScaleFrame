package org.kpc.rescaleframecraft.render.flywheel;

import com.mojang.blaze3d.vertex.*;
import java.nio.FloatBuffer;
import java.nio.charset.StandardCharsets;
import org.joml.Matrix4f;
import org.joml.Vector4f;
import org.junit.jupiter.api.*;
import org.kpc.rescaleframecraft.temporal.GeometryHistory;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.BufferUtils;
import org.lwjgl.glfw.GLFW;
import org.lwjgl.opengl.*;
import static org.junit.jupiter.api.Assertions.*;

class ActorGpuTest {
    private long window;
    @BeforeEach void context() {
        assertTrue(GLFW.glfwInit());GLFW.glfwWindowHint(GLFW.GLFW_VISIBLE,GLFW.GLFW_FALSE);
        GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MAJOR,4);GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MINOR,3);
        GLFW.glfwWindowHint(GLFW.GLFW_OPENGL_PROFILE,GLFW.GLFW_OPENGL_CORE_PROFILE);
        window=GLFW.glfwCreateWindow(64,64,"Actor motion",0,0);assertNotEquals(0,window);
        GLFW.glfwMakeContextCurrent(window);GL.createCapabilities();
    }
    @AfterEach void close() {GpuMotion.release();GLFW.glfwDestroyWindow(window);GLFW.glfwTerminate();}
    @Test void batchedActorsKeepGpuDeformationThroughSortingLateTopologyChangesAndBufferGrowth() throws Exception {
        int source=link("""
            #version 430 core
            layout(location=0) in vec3 Position;
            layout(location=1) in vec2 UV0;
            layout(location=2) in vec4 Color;
            uniform mat4 projection;
            uniform float animation;
            out vec2 uv;out vec4 tint;
            void main() {uv=UV0;tint=Color;gl_Position=projection*vec4(Position,1.0);gl_Position.x+=animation*gl_Position.w;}
            ""","""
            #version 430 core
            in vec2 uv;in vec4 tint;
            layout(location=0) out vec4 color;
            void main() {if(uv.x<0.25)discard;color=tint;}
            """);
        int merge=link(resource("fullscreen.vert"),resource("merge-flywheel.frag"));
        int vao=GL30.glGenVertexArrays(),vbo=GL15.glGenBuffers(),ebo=GL15.glGenBuffers();
        int scene=GL30.glGenFramebuffers(),result=GL30.glGenFramebuffers(),color=texture(GL30.GL_RGBA32F,GL11.GL_RGBA),output=texture(GL30.GL_RGBA32F,GL11.GL_RGBA),depth=texture(GL30.GL_DEPTH_COMPONENT32F,GL11.GL_DEPTH_COMPONENT);
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,scene);GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,color,0);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_DEPTH_ATTACHMENT,GL11.GL_TEXTURE_2D,depth,0);
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,result);GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,output,0);
        var projection=new Matrix4f().perspective(1,1,.1f,100);var camera=new TemporalCamera();camera.jitterEnabled=false;
        var history=new GeometryHistory();Object a=new Object(),b=new Object(),world=new Object();
        GL30.glBindVertexArray(vao);GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,vbo);GL15.glBindBuffer(GL15.GL_ELEMENT_ARRAY_BUFFER,ebo);
        GL11.glViewport(0,0,64,64);GL11.glDisable(GL11.GL_DEPTH_TEST);GL11.glDisable(GL11.GL_BLEND);
        try {
            for(int frame=1;frame<=4;frame++) {
                float animation=frame==1?0:frame==2?.125f:frame==3?.2f:.25f;
                float ax=frame==1?-.5f:-.375f,bx=-ax;
                GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,scene);GL20.glUseProgram(source);
                GL20.glUniformMatrix4fv(GL20.glGetUniformLocation(source,"projection"),false,projection.get(new float[16]));
                GL20.glUniform1f(GL20.glGetUniformLocation(source,"animation"),animation);
                camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);history.begin(camera.frame);
                GpuMotion.begin(camera,depth);ActorMotion.begin(camera.reset);
                try(var batch=new Recorder()) {
                    if(frame==2) {quad(history,b,batch,bx);quad(history,a,batch,ax);}
                    else {quad(history,a,batch,ax);quad(history,b,batch,bx);}
                    draw(batch,vbo,ebo,frame%2==0);
                }
                if(frame>=3)try(var late=new Recorder()) {quad(history,a,late,4);draw(late,vbo,ebo,false);}
                ActorMotion.end();GpuMotion.end();
                for(var batch:history.finish())assertTrue(batch.gpuCaptured,"Fully submitted logical vertices must not be overwritten by CPU replay");
                GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,result);GL20.glUseProgram(merge);GL11.glDisable(GL11.GL_DEPTH_TEST);
                String[] uniforms={"capturedMotion","capturedDepth","capturedPlane","capturedActors","actorValidity"};
                int[] textures={GpuMotion.motion(),GpuMotion.depth(),GpuMotion.plane(),GpuMotion.actorIds(),GpuMotion.actorValidity()};
                for(int i=0;i<5;i++) {
                    GL20.glUniform1i(GL20.glGetUniformLocation(merge,uniforms[i]),i);GL13.glActiveTexture(GL13.GL_TEXTURE0+i);
                    GL11.glBindTexture(i==4?GL31.GL_TEXTURE_BUFFER:GL11.GL_TEXTURE_2D,textures[i]);GL33.glBindSampler(i,0);
                }
                GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
                FloatBuffer pixels=BufferUtils.createFloatBuffer(64*64*4);GL11.glBindTexture(GL11.GL_TEXTURE_2D,output);GL11.glGetTexImage(GL11.GL_TEXTURE_2D,0,GL11.GL_RGBA,GL11.GL_FLOAT,pixels);
                for(float x:new float[]{ax,bx}) {
                    var clip=projection.transform(new Vector4f(x,0,-2,1));int px=(int)((clip.x/clip.w+animation+1)*32);int at=(32*64+px)*4;
                    float movement=frame==2?-(.125f+(x<0?.125f:-.125f)*projection.m00()/2)*32:frame==3?-2.4f:frame==4?-1.6f:0;
                    boolean invalid=frame==1||frame==3&&x<0;
                    assertEquals(invalid?0:movement,pixels.get(at),.025,"GPU deformation follows the object across reordered buffer slots");
                    assertEquals(invalid?1:0,pixels.get(at+2),.001,"A late topology change invalidates that object's earlier draw too");
                    if(!invalid)assertEquals(.5,pixels.get(at+3),.001);
                }
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally {
            GL20.glUseProgram(0);GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,0);GL20.glDeleteProgram(source);GL20.glDeleteProgram(merge);
            GL30.glDeleteFramebuffers(scene);GL30.glDeleteFramebuffers(result);GL11.glDeleteTextures(color);GL11.glDeleteTextures(output);GL11.glDeleteTextures(depth);
            GL30.glDeleteVertexArrays(vao);GL15.glDeleteBuffers(vbo);GL15.glDeleteBuffers(ebo);
        }
    }
    private static void quad(GeometryHistory history,Object owner,Recorder batch,float centre) {
        history.push(owner);var consumer=history.wrap(batch,null);
        try {for(int i=0;i<4;i++)consumer.addVertex(centre+(i==0||i==3?-.22f:.22f),i<2?-.22f:.22f,-2).setUv(i==0||i==3?0:1,i<2?0:1).setColor(255,255,255,255);}
        finally {history.pop();}
    }
    private static void draw(Recorder batch,int vbo,int ebo,boolean reverse) {
        var mesh=batch.builder.buildOrThrow();ActorMotion.mesh(batch.stream,mesh,batch.count);
        ActorMotion.scope(mesh,()-> {
            var format=mesh.drawState().format();GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,vbo);GL15.glBufferData(GL15.GL_ARRAY_BUFFER,mesh.vertexBuffer(),GL15.GL_STREAM_DRAW);
            GL20.glEnableVertexAttribArray(0);GL20.glVertexAttribPointer(0,3,GL11.GL_FLOAT,false,format.getVertexSize(),format.getOffset(VertexFormatElement.POSITION));
            GL20.glEnableVertexAttribArray(1);GL20.glVertexAttribPointer(1,2,GL11.GL_FLOAT,false,format.getVertexSize(),format.getOffset(VertexFormatElement.UV0));
            GL20.glEnableVertexAttribArray(2);GL20.glVertexAttribPointer(2,4,GL11.GL_UNSIGNED_BYTE,true,format.getVertexSize(),format.getOffset(VertexFormatElement.COLOR));
            int quads=batch.count/4;var indices=BufferUtils.createIntBuffer(quads*6);
            for(int q=0;q<quads;q++) {int base=(reverse?quads-1-q:q)*4;indices.put(base).put(base+1).put(base+2).put(base+2).put(base+3).put(base);}
            indices.flip();GL15.glBindBuffer(GL15.GL_ELEMENT_ARRAY_BUFFER,ebo);GL15.glBufferData(GL15.GL_ELEMENT_ARRAY_BUFFER,indices,GL15.GL_STREAM_DRAW);
            mesh.close();ActorMotion.discard(mesh);GpuMotion.drawImmediate(()->GL11.glDrawElements(GL11.GL_TRIANGLES,quads*6,GL11.GL_UNSIGNED_INT,0L));
        });
    }
    private static final class Recorder implements VertexConsumer,AutoCloseable {
        final ByteBufferBuilder buffer=new ByteBufferBuilder(1024);
        final BufferBuilder builder=new BufferBuilder(buffer,VertexFormat.Mode.QUADS,DefaultVertexFormat.POSITION_TEX_COLOR);
        ActorMotion.Stream stream;int count;
        @Override public VertexConsumer addVertex(float x,float y,float z) {builder.addVertex(x,y,z);stream=ActorMotion.vertex(stream,++count);return this;}
        @Override public VertexConsumer setColor(int r,int g,int b,int a) {builder.setColor(r,g,b,a);return this;}
        @Override public VertexConsumer setUv(float u,float v) {builder.setUv(u,v);return this;}
        @Override public VertexConsumer setUv1(int u,int v) {builder.setUv1(u,v);return this;}
        @Override public VertexConsumer setUv2(int u,int v) {builder.setUv2(u,v);return this;}
        @Override public VertexConsumer setNormal(float x,float y,float z) {builder.setNormal(x,y,z);return this;}
        @Override public void close() {buffer.close();}
    }
    private static int texture(int internal,int external) {int id=GL11.glGenTextures();GL11.glBindTexture(GL11.GL_TEXTURE_2D,id);GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,internal,64,64,0,external,GL11.GL_FLOAT,0L);GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);return id;}
    private static String resource(String name) throws Exception {try(var stream=ActorGpuTest.class.getResourceAsStream("/assets/rescaleframecraft/shaders/"+name)) {return new String(stream.readAllBytes(),StandardCharsets.UTF_8);}}
    private static int link(String vertex,String fragment) {
        int vs=compile(GL20.GL_VERTEX_SHADER,vertex),fs=compile(GL20.GL_FRAGMENT_SHADER,fragment),program=GL20.glCreateProgram();
        GL20.glAttachShader(program,vs);GL20.glAttachShader(program,fs);GL20.glLinkProgram(program);
        assertEquals(1,GL20.glGetProgrami(program,GL20.GL_LINK_STATUS),GL20.glGetProgramInfoLog(program));GL20.glDeleteShader(vs);GL20.glDeleteShader(fs);return program;
    }
    private static int compile(int type,String source) {int shader=GL20.glCreateShader(type);GL20.glShaderSource(shader,source);GL20.glCompileShader(shader);assertEquals(1,GL20.glGetShaderi(shader,GL20.GL_COMPILE_STATUS),GL20.glGetShaderInfoLog(shader));return shader;}
}
