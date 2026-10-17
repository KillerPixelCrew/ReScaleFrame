package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.FloatBuffer;
import org.junit.jupiter.api.*;
import org.lwjgl.BufferUtils;
import org.lwjgl.glfw.GLFW;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryUtil;
import static org.junit.jupiter.api.Assertions.*;

@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class FlywheelGpuTest {
    private long window;
    private GLDebugMessageCallback debug;
    @BeforeAll void context() {
        assertTrue(GLFW.glfwInit());GLFW.glfwWindowHint(GLFW.GLFW_VISIBLE,GLFW.GLFW_FALSE);
        GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MAJOR,4);GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MINOR,3);
        GLFW.glfwWindowHint(GLFW.GLFW_OPENGL_PROFILE,GLFW.GLFW_OPENGL_CORE_PROFILE);
        window=GLFW.glfwCreateWindow(64,64,"Flywheel offscreen motion",0,0);assertNotEquals(0,window);
        GLFW.glfwMakeContextCurrent(window);GL.createCapabilities();
        debug=GLDebugMessageCallback.create((source,type,id,severity,length,message,user)-> {
            System.err.println(MemoryUtil.memUTF8(message,length));
            if(type==GL43.GL_DEBUG_TYPE_ERROR)new Exception("GL call location").printStackTrace();
        });
        GL43.glDebugMessageCallback(debug,0);GL11.glEnable(GL43.GL_DEBUG_OUTPUT);GL11.glEnable(GL43.GL_DEBUG_OUTPUT_SYNCHRONOUS);
    }
    @AfterAll void close() {GL43.glDebugMessageCallback(null,0);debug.free();GLFW.glfwDestroyWindow(window);GLFW.glfwTerminate();}
    @Test void finalShaderDeformationUsesCapturedVerticesAndPreservesCutout() {
        int source=link("""
            #version 430 core
            layout(location=0) in vec2 Position;
            uniform float translation;
            uniform float animation;
            uniform vec2 jitter;
            struct FlwInstance { float x; };
            FlwInstance _flw_unpackInstance(int index) { return FlwInstance(translation); }
            uint flw_vertexId;
            out vec2 flw_vertexTexCoord;
            out vec4 flw_vertexColor;
            void main() {
                FlwInstance instance=_flw_unpackInstance(gl_InstanceID);
                flw_vertexId=uint(gl_VertexID);
                flw_vertexTexCoord=Position+vec2(0.5);
                flw_vertexColor=vec4(1.0);
                gl_Position=vec4(Position.x+instance.x,Position.y,0.0,1.0);
                // A pack may deform the result after the instance transform. { }
                gl_Position.x+=animation;
                gl_Position.xy+=jitter/32.0;
            }
            ""","""
            #version 430 core
            in vec2 flw_vertexTexCoord;
            in vec4 flw_vertexColor;
            vec4 flw_fragColor;
            layout(location=0) out vec4 originalColor;
            layout(location=1) out vec4 auxiliaryColor;
            void main() {
                flw_fragColor=flw_vertexColor;
                if(flw_vertexTexCoord.x<0.25)discard;
                originalColor=vec4(0.2,0.4,0.6,1.0);
                auxiliaryColor=vec4(1.0);
            }
            """);
        int vao=GL30.glGenVertexArrays(),vbo=GL15.glGenBuffers(),ebo=GL15.glGenBuffers(),fbo=GL30.glGenFramebuffers();
        int motion=texture(GL30.GL_RGBA32F),depth=texture(GL30.GL_R32F);
        GL30.glBindVertexArray(vao);GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,vbo);
        GL15.glBufferData(GL15.GL_ARRAY_BUFFER,new float[]{-.5f,-.5f,.5f,-.5f,.5f,.5f,-.5f,.5f},GL15.GL_STATIC_DRAW);
        GL20.glEnableVertexAttribArray(0);GL20.glVertexAttribPointer(0,2,GL11.GL_FLOAT,false,8,0);
        GL15.glBindBuffer(GL15.GL_ELEMENT_ARRAY_BUFFER,ebo);GL15.glBufferData(GL15.GL_ELEMENT_ARRAY_BUFFER,new int[]{0,1,2,2,3,0},GL15.GL_STATIC_DRAW);
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,fbo);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,motion,0);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,depth,0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1});
        assertEquals(GL30.GL_FRAMEBUFFER_COMPLETE,GL30.glCheckFramebufferStatus(GL30.GL_FRAMEBUFFER));
        try(var program=new MotionProgram(source);var first=new GpuHistoryBuffer(GL30.GL_RGBA32UI);
            var second=new GpuHistoryBuffer(GL30.GL_RGBA32UI);var indices=new GpuHistoryBuffer(GL30.GL_RG32UI);
            var meshes=new GpuHistoryBuffer(GL30.GL_R32UI)) {
            first.allocate(4*32);second.allocate(4*32);
            meshes.upload(BufferUtils.createIntBuffer(1).put(0).flip());
            GL11.glViewport(0,0,64,64);GL11.glDisable(GL11.GL_BLEND);GL11.glDisable(GL11.GL_DEPTH_TEST);
            for(int frame=1;frame<=2;frame++) {
                GL20.glUseProgram(source);GL20.glUniform1f(GL20.glGetUniformLocation(source,"translation"),frame==1?0:.25f);
                GL20.glUniform1f(GL20.glGetUniformLocation(source,"animation"),frame==1?0:.125f);
                GL20.glUniform2f(GL20.glGetUniformLocation(source,"jitter"),frame==1?-.25f:.5f,0);
                indices.upload(BufferUtils.createIntBuffer(2).put(frame==1?0:1).put(0).flip());
                program.bind();int[] textures={frame==1?second.texture():first.texture(),indices.texture(),meshes.texture()};
                for(int i=0;i<3;i++) {GL13.glActiveTexture(GL13.GL_TEXTURE0+program.textureUnits[i]);GL11.glBindTexture(GL31.GL_TEXTURE_BUFFER,textures[i]);}
                GL30.glBindBufferBase(GL43.GL_SHADER_STORAGE_BUFFER,program.storageBinding,frame==1?first.buffer():second.buffer());
                program.unsigned("_rfc_frame",frame);program.unsigned("_rfc_previousFrame",frame-1);
                program.vector("_rfc_renderSize",64,64);program.vector("_rfc_currentJitter",frame==1?-.25f:.5f,0);
                program.vector("_rfc_previousJitter",-.25f,0);
                GL30.glClearBufferfv(GL11.GL_COLOR,0,new float[]{0,0,1,-1});
                GL11.glDrawElements(GL11.GL_TRIANGLES,6,GL11.GL_UNSIGNED_INT,0L);
                GL42.glMemoryBarrier(GL43.GL_SHADER_STORAGE_BARRIER_BIT|GL42.GL_TEXTURE_FETCH_BARRIER_BIT);
            }
            FloatBuffer pixel=BufferUtils.createFloatBuffer(4);GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);
            GL11.glReadPixels(48,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
            assertEquals(-12,pixel.get(0),.01,"Retain both instance translation and later shader deformation, excluding jitter");
            assertEquals(0,pixel.get(2),.01,"Consecutive visible geometry has valid history");
            GL11.glReadPixels(27,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
            assertEquals(-1,pixel.get(3),"Original alpha discard remains a hole in the motion target");
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
        } finally {
            GL20.glUseProgram(0);GL20.glDeleteProgram(source);GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,0);
            GL30.glDeleteFramebuffers(fbo);GL11.glDeleteTextures(motion);GL11.glDeleteTextures(depth);
            GL15.glDeleteBuffers(vbo);GL15.glDeleteBuffers(ebo);GL30.glDeleteVertexArrays(vao);
        }
    }
    private static int texture(int format) {
        int id=GL11.glGenTextures();GL11.glBindTexture(GL11.GL_TEXTURE_2D,id);
        GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,64,64,0,GL11.GL_RGBA,GL11.GL_FLOAT,0L);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);return id;
    }
    private static int link(String vertex,String fragment) {
        int vs=compile(GL20.GL_VERTEX_SHADER,vertex),fs=compile(GL20.GL_FRAGMENT_SHADER,fragment),program=GL20.glCreateProgram();
        GL20.glAttachShader(program,vs);GL20.glAttachShader(program,fs);GL20.glLinkProgram(program);
        assertEquals(1,GL20.glGetProgrami(program,GL20.GL_LINK_STATUS),GL20.glGetProgramInfoLog(program));
        GL20.glDeleteShader(vs);GL20.glDeleteShader(fs);return program;
    }
    private static int compile(int type,String code) {
        int shader=GL20.glCreateShader(type);GL20.glShaderSource(shader,code);GL20.glCompileShader(shader);
        assertEquals(1,GL20.glGetShaderi(shader,GL20.GL_COMPILE_STATUS),GL20.glGetShaderInfoLog(shader));return shader;
    }
}
