package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.TextureTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.*;
import java.util.List;
import org.joml.Matrix4f;
import org.joml.Vector4f;
import org.junit.jupiter.api.*;
import org.kpc.rescaleframecraft.temporal.CloudHistory;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.glfw.GLFW;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryUtil;
import static org.junit.jupiter.api.Assertions.*;

/** Runs the production GL pipeline, without launching Minecraft or showing a window. */
@TestInstance(TestInstance.Lifecycle.PER_CLASS)
class TemporalGpuTest {
    private long window;
    @BeforeAll void context() {
        assertTrue(GLFW.glfwInit(),"GLFW initialization");
        GLFW.glfwWindowHint(GLFW.GLFW_VISIBLE,GLFW.GLFW_FALSE);
        GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MAJOR,3);
        GLFW.glfwWindowHint(GLFW.GLFW_CONTEXT_VERSION_MINOR,3);
        GLFW.glfwWindowHint(GLFW.GLFW_OPENGL_PROFILE,GLFW.GLFW_OPENGL_CORE_PROFILE);
        window=GLFW.glfwCreateWindow(64,64,"ReScaleFrameCraft offscreen tests",0,0);
        assertNotEquals(0,window,"hidden GL context creation");
        GLFW.glfwMakeContextCurrent(window);GL.createCapabilities();RenderSystem.initRenderThread();
        System.out.println("Production-shader GPU harness: "+GL11.glGetString(GL11.GL_RENDERER)+" / "+GL11.glGetString(GL11.GL_VERSION));
    }
    @AfterAll void close() { GLFW.glfwDestroyWindow(window);GLFW.glfwTerminate(); }
    @Test void allShippedProgramsLink() {
        try(var cache=new GlProgram("fullscreen.vert","taa-accumulate.frag");
            var resolve=new GlProgram("fullscreen.vert","taa-output.frag");
            var motion=new GlProgram("geometry-motion.vert","geometry-motion.frag");
            var cloud=new GlProgram("cloud-motion.vert","geometry-motion.frag");
            var sodiumCloud=new GlProgram("cloud-position-color.vert","geometry-motion.frag");
            var nativeGuides=new GlProgram("fullscreen.vert","native-guides.frag");
            var depth=new GlProgram("fullscreen.vert","normalize-depth.frag")) {
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
        }
    }
    @Test void rawEightFrameWindowAndReset() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective((float)Math.toRadians(70),1,.05f,1024);
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            for(int frame=0;frame<20;frame++) {
                // Translation must never move celestial sky, including after jitter wraps.
                camera.begin(new Matrix4f(),projection,frame*.125,0,0,64,64,world,false,false);
                float value=.05f+frame*.025f;
                target.bindWrite(true);GL11.glDisable(GL11.GL_SCISSOR_TEST);
                RenderSystem.colorMask(true,true,true,true);RenderSystem.depthMask(true);
                GL11.glClearColor(value,value*.5f,.1f,1);GL11.glClearDepth(1);
                GL11.glClear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixel=new float[4];GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
                int first=Math.max(0,frame-7);
                float expected=.05f+(first+frame)*.0125f;
                assertEquals(expected,pixel[0],.006,"raw eight-age mean at frame "+frame);
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError(),"GL error at frame "+frame);
            }
            camera.invalidate();camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);
            target.bindWrite(true);RenderSystem.colorMask(true,true,true,true);GL11.glClearColor(.9f,.2f,.1f,1);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT);
            state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
            state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
            GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
            float[] pixel=new float[4];GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
            assertEquals(.9,pixel[0],.006,"reset must reject all old color");
        } finally { target.destroyBuffers(); }
    }
    @Test void nativeGuidesFlipRowsAndConvertMotionWithoutJitter() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        int fbo=GL30.glGenFramebuffers(),vao=GL30.glGenVertexArrays();int[] output=new int[3];GL11.glGenTextures(output);
        try(var saved=new GlState();var motion=new MotionGuides();var shader=new GlProgram("fullscreen.vert","native-guides.frag")) {
            int[] formats={GL30.GL_RGBA16F,GL30.GL_R32F,GL30.GL_RG16F};
            GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,fbo);
            for(int i=0;i<3;i++) {
                TextureUnits.bind(0,output[i]);GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,formats[i],64,64,0,GL11.GL_RGBA,GL11.GL_FLOAT,0L);
                GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);
                GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MAG_FILTER,GL11.GL_NEAREST);
                GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0+i,GL11.GL_TEXTURE_2D,output[i],0);
            }
            GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2});
            assertEquals(GL30.GL_FRAMEBUFFER_COMPLETE,GL30.glCheckFramebufferStatus(GL30.GL_FRAMEBUFFER));
            motion.resize(64,64);
            for(int frame=0;frame<8;frame++) {
                camera.begin(new Matrix4f(),new Matrix4f().perspective(1,1,.05f,1024),frame,0,0,64,64,world,false,false);
                target.bindWrite(true);RenderSystem.depthMask(true);GL11.glClearDepth(frame<4?1:.5);
                GL11.glClearColor(.1f,.2f,.8f,1);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
                GL11.glEnable(GL11.GL_SCISSOR_TEST);GL11.glScissor(0,0,64,32);GL11.glClearColor(.8f,.2f,.1f,1);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT);GL11.glDisable(GL11.GL_SCISSOR_TEST);
                saved.fullscreen();motion.produce(target,camera,List.of(),new CloudHistory());
                if(frame>=4)GL30.glClearBufferfv(GL11.GL_COLOR,0,new float[]{2,3,0,.5f});
                saved.fullscreen();GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,fbo);RenderSystem.viewport(0,0,64,64);
                shader.use();shader.integer("sceneColor",0);shader.integer("sceneDepth",1);shader.integer("sceneMotion",2);
                TextureUnits.bind(0,target.getColorTextureId());TextureUnits.bind(1,motion.depth());TextureUnits.bind(2,motion.motion());
                for(int i=0;i<3;i++)GL33.glBindSampler(i,0);
                shader.matrix("inverseCurrent",camera.inverseJitteredViewProjection);shader.matrix("previousViewProjection",camera.previousViewProjection);
                shader.vector3("cameraDelta",camera.cameraDelta.x,camera.cameraDelta.y,camera.cameraDelta.z);
                shader.vector2("renderSize",64,64);shader.vector2("jitterPx",camera.jitter.x,camera.jitter.y);shader.integer("resetHistory",0);
                GL30.glBindVertexArray(vao);GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
                GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);float[] pixel=new float[4];GL11.glReadPixels(32,8,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
                assertEquals(.1,pixel[0],.006,"DX first rows must contain GL upper image");assertEquals(.8,pixel[2],.006);
                GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT2);GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
                assertEquals(frame<4?0:-2,pixel[0],.003,"stationary sky excludes eye translation and raster jitter; object X changes sign");
                assertEquals(frame<4?0:3,pixel[1],.003,"DX row flip and temporal direction cancel for object Y");
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally {GL11.glDeleteTextures(output);GL30.glDeleteFramebuffers(fbo);GL30.glDeleteVertexArrays(vao);target.destroyBuffers();}
    }
    @Test void homogeneousSkySurvivesEyeOffsetAndInfiniteFar() {
        for(boolean z01:new boolean[]{false,true})for(boolean reversed:new boolean[]{false,true})
        for(float far:new float[]{1024,Float.POSITIVE_INFINITY}) {
            var canonical=new Matrix4f().perspective(1.2f,16f/9,.05f,far);
            var depthRemap=TemporalCamera.canonicalDepth(reversed,z01);
            var nativeProjection=new Matrix4f(depthRemap).invert().mul(canonical);
            var camera=new TemporalCamera();var world=new Object();
            for(int frame=0;frame<16;frame++) {
                camera.begin(new Matrix4f(),nativeProjection,frame*.2,0,0,1920,1080,world,reversed,z01);
                for(float x:new float[]{-.9f,0,.9f})for(float y:new float[]{-.9f,0,.9f}) {
                    // Deliberately include a translated inverse, as in stereo eye transforms.
                    var inverse=new Matrix4f().translate(.032f,2,5).mul(camera.inverseJitteredViewProjection);
                    var ray=TemporalCamera.skyDirection(inverse,x,y);
                    var clip=camera.previousViewProjection.transform(new Vector4f(ray,0));
                    float px=(clip.x/clip.w*.5f+.5f)*1920;
                    float py=(clip.y/clip.w*.5f+.5f)*1080;
                    assertEquals((x*.5f+.5f)*1920-camera.jitter.x,px,.002);
                    assertEquals((y*.5f+.5f)*1080-camera.jitter.y,py,.002);
                    assertTrue(Float.isFinite(px)&&Float.isFinite(py));
                }
            }
        }
    }
    @Test void normalizedDepthMatchesWriterDepth() {
        var target=new TextureTarget(64,64,true,false);
        try(var state=new GlState();var guides=new MotionGuides()) {
            guides.resize(64,64);
            for(boolean reversed:new boolean[]{false,true})for(boolean z01:new boolean[]{false,true}) {
                var camera=new TemporalCamera();
                camera.begin(new Matrix4f(),new Matrix4f(),0,0,0,64,64,new Object(),reversed,z01);
                target.bindWrite(true);RenderSystem.depthMask(true);GL11.glClearDepth(reversed?.2:.8);
                GL11.glClear(GL11.GL_DEPTH_BUFFER_BIT);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT1);
                float[] value=new float[1];GL11.glReadPixels(32,32,1,1,GL11.GL_RED,GL11.GL_FLOAT,value);
                assertEquals(.8,value[0],.00001,"canonical window depth, reversed="+reversed+", z01="+z01);
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally { target.destroyBuffers();GL11.glClearDepth(1); }
    }
    @Test void actualMotionShaderWritesUnjitteredPixelDisplacement() {
        var target=new TextureTarget(64,64,true,false);
        int vao=GL30.glGenVertexArrays(),vbo=GL15.glGenBuffers();
        try(var state=new GlState();var program=new GlProgram("geometry-motion.vert","geometry-motion.frag")) {
            target.bindWrite(true);state.fullscreen();
            GL30.glBindVertexArray(vao);GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,vbo);
            // Current mesh has moved right by 0.1 clip units; old mesh did not.
            float[] points={-.5f,-.5f,.5f,-.5f,0,.5f};
            var vertices=MemoryUtil.memAllocFloat(30);
            try {
                for(int i=0;i<3;i++)vertices.put(points[i*2]+.1f).put(points[i*2+1]).put(0)
                    .put(points[i*2]).put(points[i*2+1]).put(0).put(0).put(0).put(1).put(0);
                vertices.flip();GL15.glBufferData(GL15.GL_ARRAY_BUFFER,vertices,GL15.GL_STATIC_DRAW);
            } finally { MemoryUtil.memFree(vertices); }
            int[] sizes={3,3,2,2};int offset=0;
            for(int i=0;i<4;i++) { GL20.glEnableVertexAttribArray(i);GL20.glVertexAttribPointer(i,sizes[i],GL11.GL_FLOAT,false,40,offset);offset+=sizes[i]*4; }
            program.use();program.matrix("currentViewProjection",new Matrix4f().translate(.25f/32,-.125f/32,0));
            program.matrix("previousViewProjection",new Matrix4f());program.matrix("textureMatrix",new Matrix4f());
            program.vector2("renderSize",64,64);program.vector2("jitterPx",.25f,-.125f);program.integer("textured",0);
            // TextureTarget is normalized RGBA8: use a signed float output texture.
            int motion=GL11.glGenTextures();TextureUnits.bind(0,motion);
            GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,GL30.GL_RGBA32F,64,64,0,GL11.GL_RGBA,GL11.GL_FLOAT,0L);
            GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,motion,0);
            GL11.glDrawBuffer(GL30.GL_COLOR_ATTACHMENT0);GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);
            GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
            float[] pixel=new float[4];GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
            // Raster interpolation has driver-dependent subpixel precision.
            assertEquals(-3.2,pixel[0],.002,"previous minus current in unjittered pixels");
            assertEquals(0,pixel[1],.0001);assertEquals(0,pixel[2],.0001);assertEquals(.5,pixel[3],.0001);
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,target.getColorTextureId(),0);
            com.mojang.blaze3d.platform.GlStateManager._deleteTexture(motion);
        } finally { GL30.glDeleteVertexArrays(vao);GL15.glDeleteBuffers(vbo);target.destroyBuffers(); }
    }
    @Test void cameraCutsResizeAndWorldChangesResetHistory() {
        var camera=new TemporalCamera();var world=new Object();
        var p=new Matrix4f().perspective(1,1,.1f,1000);
        camera.begin(new Matrix4f(),p,0,0,0,64,64,world,false,false);assertTrue(camera.reset);
        camera.begin(new Matrix4f(),p,0,0,0,64,64,world,false,false);assertFalse(camera.reset);
        camera.begin(new Matrix4f(),p,0,0,0,128,64,world,false,false);assertTrue(camera.reset);
        camera.begin(new Matrix4f(),p,0,0,0,128,64,new Object(),false,false);assertTrue(camera.reset);
        camera.begin(new Matrix4f(),p,20,0,0,128,64,world,false,false);assertTrue(camera.reset);
    }
    @Test void patternedSkyStaysStillAcrossJitterAndCameraTranslation() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        var input=MemoryUtil.memAllocFloat(64*64*4);
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            for(int frame=0;frame<24;frame++) {
                camera.begin(new Matrix4f(),projection,frame*.125,frame*.05,frame*.2,64,64,world,false,false);
                // An eye/view origin embedded in the inverse must cancel from sky direction.
                camera.inverseJitteredViewProjection.set(new Matrix4f().translate(.032f,2,5).mul(camera.inverseJitteredViewProjection));
                input.clear();
                for(int y=0;y<64;y++)for(int x=0;x<64;x++) {
                    float c=skyPattern(x+.5f-camera.jitter.x,y+.5f-camera.jitter.y);
                    input.put(c).put(c*.5f).put(.1f).put(1);
                }
                input.flip();TextureUnits.bind(0,target.getColorTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,input);
                target.bindWrite(true);RenderSystem.depthMask(true);GL11.glClearDepth(1);GL11.glClear(GL11.GL_DEPTH_BUFFER_BIT);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixels=new float[64*64*4];GL11.glReadPixels(0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,pixels);
                for(int y=8;y<56;y+=7)for(int x=8;x<56;x+=7)
                    assertEquals(skyPattern(x+.5f,y+.5f),pixels[(y*64+x)*4],.012,"sky drift frame "+frame+" at "+x+","+y);
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally { MemoryUtil.memFree(input);target.destroyBuffers(); }
    }
    private static float skyPattern(float x,float y) { return .5f+.35f*(float)(Math.sin(x*.2)*Math.cos(y*.17)); }
    @Test void stationaryCutoutSilhouetteMustNotOscillateWithJitter() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        var clip=projection.transform(new Vector4f(0,0,-30,1));
        float foliageDepth=clip.z/clip.w*.5f+.5f;
        var input=MemoryUtil.memAllocFloat(64*64*4);var depth=MemoryUtil.memAllocFloat(64*64);
        float[] minimum=new float[64*64],maximum=new float[64*64];
        java.util.Arrays.fill(minimum,Float.POSITIVE_INFINITY);
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            for(int frame=0;frame<40;frame++) {
                camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);
                input.clear();depth.clear();
                for(int y=0;y<64;y++)for(int x=0;x<64;x++) {
                    float u=x+.5f-camera.jitter.x;
                    boolean vine=u>=20.1f&&u<21.35f||u>=39.2f&&u<40.45f;
                    input.put(vine?.05f:.8f).put(vine?.4f:.7f).put(vine?.1f:.95f).put(1);
                    depth.put(vine?foliageDepth:1);
                }
                input.flip();depth.flip();TextureUnits.bind(0,target.getColorTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,input);
                TextureUnits.bind(0,target.getDepthTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_DEPTH_COMPONENT,GL11.GL_FLOAT,depth);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixels=new float[64*64*4];GL11.glReadPixels(0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,pixels);
                if(frame>=24)for(int y=8;y<56;y++)for(int x=8;x<56;x++) {
                    int i=y*64+x;minimum[i]=Math.min(minimum[i],pixels[i*4]);maximum[i]=Math.max(maximum[i],pixels[i*4]);
                }
            }
            float oscillation=0;int worst=0;
            for(int y=8;y<56;y++)for(int x=8;x<56;x++)if(maximum[y*64+x]-minimum[y*64+x]>oscillation) { worst=y*64+x;oscillation=maximum[worst]-minimum[worst]; }
            System.out.println("Stationary cutout max temporal oscillation: "+oscillation);
            if(oscillation>=.008) {
                System.out.println("Worst cutout pixel "+worst%64+","+worst/64);
            }
            assertTrue(oscillation<.008,"stable eight-phase cutout window must not oscillate; observed "+oscillation);
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
        } finally { MemoryUtil.memFree(input);MemoryUtil.memFree(depth);target.destroyBuffers(); }
    }
    @Test void skyDisocclusionMustRejectTheActualOldOccluder() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        var clip=projection.transform(new Vector4f(0,0,-30,1));float objectDepth=clip.z/clip.w*.5f+.5f;
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            for(int frame=0;frame<12;frame++) {
                camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);
                boolean revealed=frame>=10;float color=revealed?.8f:.05f;
                target.bindWrite(true);RenderSystem.colorMask(true,true,true,true);RenderSystem.depthMask(true);
                GL11.glClearColor(color,color,color,1);GL11.glClearDepth(revealed?1:objectDepth);
                GL11.glClear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixel=new float[4];GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,pixel);
                assertEquals(color,pixel[0],.006,"old foreground must not trail into newly revealed sky, frame "+frame);
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally { target.destroyBuffers();GL11.glClearDepth(1); }
    }
    @Test void hudCompositionCannotModifySceneOutputOrFeedHistory() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            for(int frame=0;frame<12;frame++) {
                camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);
                target.bindWrite(true);RenderSystem.colorMask(true,true,true,true);RenderSystem.depthMask(true);
                GL11.glDisable(GL11.GL_SCISSOR_TEST);GL11.glClearColor(.2f,.4f,.6f,1);GL11.glClearDepth(1);
                GL11.glClear(GL11.GL_COLOR_BUFFER_BIT|GL11.GL_DEPTH_BUFFER_BIT);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                assertNotEquals(target.getColorTextureId(),history.sceneColor(),"scene output must have separate ownership");
                // An ordinary overlay is drawn after the production resolve and blit.
                target.bindWrite(true);GL11.glEnable(GL11.GL_SCISSOR_TEST);GL11.glScissor(0,0,8,8);
                GL11.glClearColor(1,0,1,1);GL11.glClear(GL11.GL_COLOR_BUFFER_BIT);GL11.glDisable(GL11.GL_SCISSOR_TEST);
                float[] composed=new float[4];GL11.glReadPixels(3,3,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,composed);
                assertEquals(1,composed[0],.006);assertEquals(0,composed[1],.006);
                TextureUnits.bind(0,history.sceneColor());float[] clean=new float[64*64*4];
                GL11.glGetTexImage(GL11.GL_TEXTURE_2D,0,GL11.GL_RGBA,GL11.GL_FLOAT,clean);
                int i=(3*64+3)*4;
                assertEquals(.2,clean[i],.006,"HUD must never enter retained scene output/history, frame "+frame);
                assertEquals(.4,clean[i+1],.006);assertEquals(.6,clean[i+2],.006);
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally { target.destroyBuffers(); }
    }
    @Test void movingCutoutsDoNotLeaveDistantSkyTrails() { movingCutoutScene(false); }
    @Test void movingCloudsBehindVinesUseTheirOwnMotion() { movingCutoutScene(true); }
    private void movingCutoutScene(boolean clouds) {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        var clip=projection.transform(new Vector4f(0,0,-30,1));float foliageDepth=clip.z/clip.w*.5f+.5f;
        var cloudClip=projection.transform(new Vector4f(0,0,-150,1));float cloudDepth=cloudClip.z/cloudClip.w*.5f+.5f;
        var input=MemoryUtil.memAllocFloat(64*64*4);var depth=MemoryUtil.memAllocFloat(64*64);
        var cloudMotion=MemoryUtil.memAllocFloat(64*64*4);
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);
            float worst=0;String worstSample="";
            for(int frame=0;frame<36;frame++) {
                float shift=Math.max(0,frame-12)*.7f;
                camera.begin(new Matrix4f(),projection,shift*30/(32*projection.m00()),0,0,64,64,world,false,false);
                input.clear();depth.clear();cloudMotion.clear();
                for(int y=0;y<64;y++)for(int x=0;x<64;x++) {
                    float u=x+.5f-camera.jitter.x+shift;
                    boolean vine=u>=28.1f&&u<29.35f||u>=43.2f&&u<49.45f;
                    float bg=clouds?cloudPattern(x+.5f-camera.jitter.x+shift*.2f+frame*.2f):.8f;
                    input.put(vine?.05f:bg).put(vine?.4f:.7f).put(vine?.1f:.95f).put(1);depth.put(vine?foliageDepth:clouds?cloudDepth:1);
                    cloudMotion.put(vine?0:.2f+(frame>12?.14f:0)).put(0).put(vine?0:.2f).put(vine?0:cloudDepth);
                }
                input.flip();depth.flip();TextureUnits.bind(0,target.getColorTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,input);
                TextureUnits.bind(0,target.getDepthTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_DEPTH_COMPONENT,GL11.GL_FLOAT,depth);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                if(clouds) {
                    cloudMotion.flip();TextureUnits.bind(0,guides.motion());
                    GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,cloudMotion);
                }
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixels=new float[64*64*4];GL11.glReadPixels(0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,pixels);
                if(frame>=13)for(int x=8;x<56;x++) {
                    float u=x+.5f+shift;
                    float margin=2;
                    if(u<28.1f-margin||u>29.35f+margin&&u<43.2f-margin||u>49.45f+margin) {
                        float expected=clouds?cloudPattern(x+.5f+shift*.2f+frame*.2f):.8f;
                        float error=Math.abs(expected-pixels[(32*64+x)*4]);
                        if(error>worst) { worst=error;worstSample="frame="+frame+" x="+x+" foregroundU="+u+" expected="+expected+" actual="+pixels[(32*64+x)*4]; }
                    }
                }
            }
            System.out.println("Moving cutout background error (clouds="+clouds+"): "+worst);
            if(worst>=.012)System.out.println(worstSample);
            assertTrue(worst<.012,"foreground history must not trail across clear sky; observed "+worst);
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
        } finally { MemoryUtil.memFree(input);MemoryUtil.memFree(depth);MemoryUtil.memFree(cloudMotion);target.destroyBuffers(); }
    }
    private static float cloudPattern(float x) { return .65f+.2f*(float)Math.sin(x*.3); }
    @Test void cameraPanDoesNotSmearVinesAcrossSky() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();
        var projection=new Matrix4f().perspective(1.2f,1,.05f,1024);
        var input=MemoryUtil.memAllocFloat(64*64*4);var depth=MemoryUtil.memAllocFloat(64*64);
        float[] edges={28.1f,29.35f,43.2f,49.45f};
        try(var state=new GlState();var history=new TaaHistory();var guides=new MotionGuides()) {
            history.resize(64,64);guides.resize(64,64);float worst=0;
            for(int frame=0;frame<36;frame++) {
                float yaw=Math.max(0,frame-12)*.009f;
                camera.begin(new Matrix4f().rotateY(yaw),projection,0,0,0,64,64,world,false,false);
                input.clear();depth.clear();
                for(int y=0;y<64;y++)for(int x=0;x<64;x++) {
                    var ray=camera.inverseJitteredViewProjection.transform(new Vector4f((x+.5f)/32-1,(y+.5f)/32-1,1,1));
                    float worldX=ray.x/(-ray.z)*30,worldY=ray.y/(-ray.z)*30;
                    float u=worldX*projection.m00()*32/30+32;
                    boolean vine=u>=edges[0]&&u<edges[1]||u>=edges[2]&&u<edges[3];
                    var clip=camera.jitteredViewProjection.transform(new Vector4f(worldX,worldY,-30,1));
                    input.put(vine?.05f:.8f).put(vine?.4f:.7f).put(vine?.1f:.95f).put(1);
                    depth.put(vine?clip.z/clip.w*.5f+.5f:1);
                }
                input.flip();depth.flip();TextureUnits.bind(0,target.getColorTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,input);
                TextureUnits.bind(0,target.getDepthTextureId());
                GL11.glTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,64,64,GL11.GL_DEPTH_COMPONENT,GL11.GL_FLOAT,depth);
                state.fullscreen();guides.produce(target,camera,List.of(),new CloudHistory());
                state.fullscreen();history.resolve(target,camera,guides.depth(),guides.motion());
                float[] projected=new float[4];
                for(int i=0;i<4;i++) {
                    var h=camera.viewProjection.transform(new Vector4f((edges[i]-32)*30/(32*projection.m00()),0,-30,1));
                    projected[i]=(h.x/h.w*.5f+.5f)*64;
                }
                GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
                float[] pixels=new float[64*64*4];GL11.glReadPixels(0,0,64,64,GL11.GL_RGBA,GL11.GL_FLOAT,pixels);
                if(frame>=13)for(int x=8;x<56;x++) {
                    float p=x+.5f;
                    if(p<projected[0]-2||p>projected[1]+2&&p<projected[2]-2||p>projected[3]+2)
                        worst=Math.max(worst,Math.abs(.8f-pixels[(32*64+x)*4]));
                }
            }
            System.out.println("Camera-pan cutout trail error: "+worst);
            assertTrue(worst<.012,"camera pan must not leave a multi-frame vine trail; observed "+worst);
            assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
        } finally { MemoryUtil.memFree(input);MemoryUtil.memFree(depth);target.destroyBuffers(); }
    }
    @Test void sodiumCloudMeshProducesMotionAndInvalidatesAfterUpload() {
        var target=new TextureTarget(64,64,true,false);
        var camera=new TemporalCamera();var world=new Object();var clouds=new CloudHistory();
        var projection=new Matrix4f().ortho(-2,2,-2,2,.1f,20);
        try(var state=new GlState();var guides=new MotionGuides();var source=new GlProgram("cloud-source.vert","cloud-source.frag");
            var memory=new ByteBufferBuilder(1024);var mesh=new VertexBuffer(VertexBuffer.Usage.DYNAMIC)) {
            var builder=new BufferBuilder(memory,VertexFormat.Mode.QUADS,DefaultVertexFormat.POSITION_COLOR);
            builder.addVertex(-1,-1,0).setColor(255,255,255,255);builder.addVertex(1,-1,0).setColor(255,255,255,255);
            builder.addVertex(1,1,0).setColor(255,255,255,255);builder.addVertex(-1,1,0).setColor(255,255,255,255);
            mesh.bind();mesh.upload(builder.buildOrThrow());VertexBuffer.unbind();guides.resize(64,64);
            for(int frame=0;frame<4;frame++) {
                camera.begin(new Matrix4f(),projection,0,0,0,64,64,world,false,false);
                var model=new Matrix4f().translate(frame==0?0:.1f,0,-2);
                if(frame==2)clouds.meshChanged(mesh);
                target.bindWrite(true);RenderSystem.colorMask(true,true,true,true);RenderSystem.depthMask(true);
                RenderSystem.disableCull();RenderSystem.disableBlend();RenderSystem.enableDepthTest();RenderSystem.depthFunc(GL11.GL_ALWAYS);
                GL11.glClearDepth(1);GL11.glClear(GL11.GL_DEPTH_BUFFER_BIT);
                source.use();source.matrix("projection",camera.jitteredProjection);source.matrix("modelView",model);
                mesh.bind();mesh.draw();VertexBuffer.unbind();
                if(frame==3) {
                    GL11.glEnable(GL11.GL_SCISSOR_TEST);GL11.glScissor(28,28,8,8);GL11.glClearDepth(.01);
                    GL11.glClear(GL11.GL_DEPTH_BUFFER_BIT);GL11.glDisable(GL11.GL_SCISSOR_TEST);GL11.glClearDepth(1);
                }
                clouds.capture(mesh,model,camera.jitteredProjection,0,.8f,camera);
                assertEquals(frame==1||frame==3,clouds.valid,"cloud vertex-buffer generation must match previous geometry");
                state.fullscreen();guides.produce(target,camera,List.of(),clouds);
                GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);float[] motion=new float[4];
                GL11.glReadPixels(32,32,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,motion);
                if(frame==3) {
                    assertEquals(0,motion[3],.00001,"cloud motion must not overwrite a nearer occluder");
                    GL11.glReadPixels(24,24,1,1,GL11.GL_RGBA,GL11.GL_FLOAT,motion);
                    assertTrue(motion[3]>0,"unoccluded cloud fragments must still write motion");
                    assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());continue;
                }
                assertTrue(motion[3]>0,"Sodium position/colour geometry must actually write cloud motion");
                if(frame==1) {
                    assertEquals(-1.6,motion[0],.004,"actual cloud drift, excluding raster jitter");
                    assertEquals(0,motion[1],.004);assertEquals(.2,motion[2],.002);
                } else assertEquals(1,motion[2],.002,"missing previous geometry must reject old cloud history");
                assertEquals(GL11.GL_NO_ERROR,GL11.glGetError());
            }
        } finally { target.destroyBuffers(); }
    }
}
