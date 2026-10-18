package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.VertexFormat;
import java.nio.FloatBuffer;
import java.util.List;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.temporal.GeometryHistory;
import org.kpc.rescaleframecraft.temporal.CloudHistory;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.kpc.rescaleframecraft.render.flywheel.GpuMotion;
import com.mojang.blaze3d.vertex.VertexFormatElement;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryUtil;

final class MotionGuides implements AutoCloseable {
    private final GlProgram normalize=new GlProgram("fullscreen.vert","normalize-depth.frag");
    private final GlProgram resolveDepth=new GlProgram("fullscreen.vert","resolve-distant-depth.frag");
    private final GlProgram mergeFlywheel=new GlProgram("fullscreen.vert","merge-flywheel.frag");
    private final GlProgram geometry=new GlProgram("geometry-motion.vert","geometry-motion.frag");
    private final GlProgram clouds=new GlProgram("cloud-motion.vert","geometry-motion.frag");
    private final GlProgram sodiumClouds=new GlProgram("cloud-position-color.vert","geometry-motion.frag");
    private final int fbo=GL30.glGenFramebuffers(),vao=GL30.glGenVertexArrays(),vbo=GL15.glGenBuffers();
    private int width,height,motion,depth,plane,testDepth;
    private int resolvedFbo,resolvedMotion,resolvedDepth,resolvedPlane;
    private boolean resolved;
    private int sourceDepthId=-1,testDepthFormat=GL30.GL_DEPTH_COMPONENT32F;
    private FloatBuffer vertices=MemoryUtil.memAllocFloat(65536);
    private final Matrix4f distantInverse=new Matrix4f(),sceneProjection=new Matrix4f(),sourceInverse=new Matrix4f();
    int motion() { return resolved?resolvedMotion:motion; }
    int depth() { return resolved?resolvedDepth:depth; }
    int plane() { return resolved?resolvedPlane:plane; }
    void resize(int w,int h) {
        if(w==width&&h==height)return;
        deleteTextures();width=w;height=h;sourceDepthId=-1;testDepthFormat=GL30.GL_DEPTH_COMPONENT32F;
        motion=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);depth=texture(GL30.GL_R32F,GL11.GL_RED);plane=texture(GL30.GL_RG16F,GL30.GL_RG);
        testDepth=texture(GL30.GL_DEPTH_COMPONENT32F,GL11.GL_DEPTH_COMPONENT);
        bind();
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,motion,0);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,depth,0);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT2,GL11.GL_TEXTURE_2D,plane,0);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_DEPTH_ATTACHMENT,GL11.GL_TEXTURE_2D,testDepth,0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2});
        if(GL30.glCheckFramebufferStatus(GL30.GL_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)
            throw new IllegalStateException("Incomplete temporal motion framebuffer");
    }
    void produce(RenderTarget source,TemporalCamera camera,List<GeometryHistory.Batch> draws,CloudHistory cloud) {
        resolved=false;
        matchDepthPrecision(source.getDepthTextureId());
        bind();RenderSystem.viewport(0,0,width,height);
        RenderSystem.enableDepthTest();RenderSystem.depthFunc(GL11.GL_ALWAYS);RenderSystem.depthMask(true);
        normalize.use();normalize.integer("sourceDepth",0);normalize.integer("reversedDepth",camera.reversedDepth?1:0);
        RenderSystem.activeTexture(GL13.GL_TEXTURE0);GL33.glBindSampler(0,0);RenderSystem.bindTexture(source.getDepthTextureId());
        GL30.glBindVertexArray(vao);GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
        if(GL.getCapabilities().OpenGL45||GL.getCapabilities().GL_ARB_clip_control)
            GL45.glClipControl(GL45.GL_LOWER_LEFT,GL45.GL_NEGATIVE_ONE_TO_ONE);
        if(GpuMotion.present()) {
            RenderSystem.depthFunc(GL11.GL_LEQUAL);
            mergeFlywheel.use();mergeFlywheel.integer("capturedMotion",0);mergeFlywheel.integer("capturedDepth",1);mergeFlywheel.integer("capturedPlane",2);
            TextureUnits.bind(0,GpuMotion.motion());TextureUnits.bind(1,GpuMotion.depth());
            TextureUnits.bind(2,GpuMotion.plane());GL33.glBindSampler(2,0);
            mergeFlywheel.integer("capturedActors",3);mergeFlywheel.integer("actorValidity",4);
            TextureUnits.bind(3,GpuMotion.actorIds());TextureUnits.buffer(4,GpuMotion.actorValidity());
            GL33.glBindSampler(3,0);GL33.glBindSampler(4,0);
            GL33.glBindSampler(0,0);GL33.glBindSampler(1,0);GL30.glBindVertexArray(vao);
            GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
        }
        for(GeometryHistory.Batch batch:draws)if(!batch.gpuCaptured)render(batch,camera);
        if(cloud.present(camera.frame)) {
            bind();RenderSystem.enableDepthTest();RenderSystem.depthFunc(GL11.GL_LEQUAL);RenderSystem.depthMask(true);
            RenderSystem.disableCull();RenderSystem.disableBlend();RenderSystem.colorMask(true,true,true,true);
            GlProgram program=cloud.texture==0?sodiumClouds:clouds;
            program.use();program.matrix("currentProjection",cloud.projection);program.matrix("currentModelView",cloud.modelView);
            program.matrix("previousViewProjection",cloud.previous);
            program.integer("reversedDepth",camera.reversedDepth?1:0);program.integer("zeroToOne",camera.zeroToOne?1:0);
            program.vector2("renderSize",width,height);program.vector2("jitterPx",camera.jitter.x,camera.jitter.y);
            program.integer("validHistory",cloud.valid?1:0);program.integer("textured",cloud.texture==0?0:1);program.integer("colorTexture",0);
            program.scalar("cloudAlpha",cloud.alpha);
            RenderSystem.activeTexture(GL13.GL_TEXTURE0);RenderSystem.bindTexture(cloud.texture);GL33.glBindSampler(0,0);
            cloud.buffer.bind();cloud.buffer.draw();
        }
        RenderSystem.disableDepthTest();RenderSystem.depthMask(false);
    }
    void resolveDistantDepth(TemporalCamera source,TemporalCamera destination) {
        if(resolvedFbo==0) {
            resolvedFbo=GL30.glGenFramebuffers();GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,resolvedFbo);
            resolvedMotion=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);resolvedDepth=texture(GL30.GL_R32F,GL11.GL_RED);resolvedPlane=texture(GL30.GL_RG16F,GL30.GL_RG);
            GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,resolvedMotion,0);
            GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,resolvedDepth,0);
            GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT2,GL11.GL_TEXTURE_2D,resolvedPlane,0);
            GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2});
            if(GL30.glCheckFramebufferStatus(GL30.GL_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Incomplete distant depth resolve framebuffer");
        }
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,resolvedFbo);RenderSystem.viewport(0,0,width,height);
        RenderSystem.disableDepthTest();RenderSystem.depthMask(false);RenderSystem.disableBlend();RenderSystem.disableCull();RenderSystem.colorMask(true,true,true,true);
        int distant=DistantHorizonsIntegration.depth();
        resolveDepth.use();resolveDepth.integer("sourceDepth",0);resolveDepth.integer("sourceMotion",1);
        resolveDepth.integer("sourcePlane",3);
        resolveDepth.integer("distantDepth",2);resolveDepth.integer("useDistantDepth",distant!=0?1:0);
        TemporalCamera.canonicalDepth(destination.reversedDepth,destination.zeroToOne,sceneProjection).mul(destination.jitteredProjection);
        TemporalCamera.canonicalDepth(source.reversedDepth,source.zeroToOne,sourceInverse).mul(source.jitteredProjection).invert();
        resolveDepth.matrix("sourceToGuide",distantInverse.set(sceneProjection).mul(sourceInverse));
        if(distant!=0) {DistantHorizonsIntegration.inverseProjection(distantInverse);resolveDepth.matrix("distantToGuide",distantInverse.mulLocal(sceneProjection));}
        TextureUnits.bind(0,depth);TextureUnits.bind(1,motion);TextureUnits.bind(2,distant);TextureUnits.bind(3,plane);
        for(int i=0;i<4;i++)GL33.glBindSampler(i,0);
        GL30.glBindVertexArray(vao);GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);resolved=true;
    }
    private void render(GeometryHistory.Batch batch,TemporalCamera camera) {
        if(batch.vertices()==0)return;
        batch.type.setupRenderState();
        var original=RenderSystem.getShader();
        // Resolve the material's textures through Minecraft, then release its
        // program cache before binding our own program.
        if(original!=null) { original.setDefaultUniforms(batch.type.mode(),camera.view,camera.jitteredProjection,net.minecraft.client.Minecraft.getInstance().getWindow());original.apply();original.clear(); }
        bind();RenderSystem.disableBlend();RenderSystem.disableCull();
        RenderSystem.colorMask(true,true,true,true);
        RenderSystem.enableDepthTest();RenderSystem.depthFunc(GL11.GL_LEQUAL);RenderSystem.depthMask(true);
        geometry.use();geometry.matrix("currentViewProjection",camera.jitteredViewProjection);
        geometry.matrix("previousViewProjection",camera.previousViewProjection);
        geometry.vector2("renderSize",width,height);geometry.vector2("jitterPx",camera.jitter.x,camera.jitter.y);
        geometry.integer("colorTexture",0);
        geometry.matrix("textureMatrix",RenderSystem.getTextureMatrix());
        // Iris binds material textures through its own sampler table. Vanilla's
        // Sampler0 name is not a material-coverage contract under shader packs.
        geometry.integer("textured",batch.type.format().getOffset(VertexFormatElement.UV0)>=0?1:0);
        RenderSystem.activeTexture(GL13.GL_TEXTURE0);GL33.glBindSampler(0,0);
        RenderSystem.bindTexture(RenderSystem.getShaderTexture(0));
        boolean previous=batch.hasPrevious(camera.frame)&&!camera.reset;
        int count=batch.vertices();boolean quads=batch.type.mode()==VertexFormat.Mode.QUADS;
        int output=quads?(count/4)*6:count;
        int floats=output*10;
        if(vertices.capacity()<floats) { MemoryUtil.memFree(vertices);vertices=MemoryUtil.memAllocFloat(Integer.highestOneBit(floats)*2); }
        vertices.clear();
        int[] order={0,1,2,2,3,0};
        for(int v=0;v<output;v++) {
            int index=(quads?(v/6)*4+order[v%6]:v)*GeometryHistory.STRIDE;
            float[] now=batch.current,old=previous?batch.previous:now;
            float reactive=previous&&now[index+3]==old[index+3]&&now[index+4]==old[index+4]&&now[index+6]==old[index+6]?0:1;
            vertices.put(now,index,3);vertices.put(old,index,3);
            vertices.put(now[index+3]).put(now[index+4]).put(now[index+5]).put(reactive);
        }
        vertices.flip();GL30.glBindVertexArray(vao);GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,vbo);
        GL15.glBufferData(GL15.GL_ARRAY_BUFFER,vertices,GL15.GL_STREAM_DRAW);
        int[] sizes={3,3,2,2};int offset=0;
        for(int i=0;i<4;i++) { GL20.glEnableVertexAttribArray(i);GL20.glVertexAttribPointer(i,sizes[i],GL11.GL_FLOAT,false,40,offset);offset+=sizes[i]*4; }
        GL11.glDrawArrays(quads?GL11.GL_TRIANGLES:batch.type.mode().asGLMode,0,output);
        batch.type.clearRenderState();
    }
    private void bind() { GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,fbo); }
    private void matchDepthPrecision(int source) {
        if(source==sourceDepthId)return;
        TextureUnits.bind(0,source);
        int format=GL11.glGetTexLevelParameteri(GL11.GL_TEXTURE_2D,0,GL11.GL_TEXTURE_INTERNAL_FORMAT);
        if(format==GL30.GL_DEPTH32F_STENCIL8)format=GL30.GL_DEPTH_COMPONENT32F;
        else if(format==GL30.GL_DEPTH24_STENCIL8)format=GL14.GL_DEPTH_COMPONENT24;
        else if(format==GL11.GL_DEPTH_COMPONENT) {
            int bits=GL11.glGetTexLevelParameteri(GL11.GL_TEXTURE_2D,0,GL14.GL_TEXTURE_DEPTH_SIZE);
            format=bits<=16?GL14.GL_DEPTH_COMPONENT16:bits<=24?GL14.GL_DEPTH_COMPONENT24:GL14.GL_DEPTH_COMPONENT32;
        }
        if(format!=testDepthFormat) {
            // Equal-depth replay must quantize exactly like the original draw.
            // D24 source depth compared with D32F replay otherwise rejects the
            // very surface whose movement is being written.
            TextureUnits.bind(0,testDepth);
            GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,width,height,0,GL11.GL_DEPTH_COMPONENT,GL11.GL_FLOAT,0L);
            testDepthFormat=format;
        }
        sourceDepthId=source;
    }
    private int texture(int format,int external) {
        int id=GL11.glGenTextures();RenderSystem.bindTexture(id);
        GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,width,height,0,external,GL11.GL_FLOAT,0L);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MAG_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_WRAP_S,GL12.GL_CLAMP_TO_EDGE);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_WRAP_T,GL12.GL_CLAMP_TO_EDGE);return id;
    }
    private void deleteTextures() {
        com.mojang.blaze3d.platform.GlStateManager._deleteTexture(motion);com.mojang.blaze3d.platform.GlStateManager._deleteTexture(depth);com.mojang.blaze3d.platform.GlStateManager._deleteTexture(testDepth);
        com.mojang.blaze3d.platform.GlStateManager._deleteTexture(plane);com.mojang.blaze3d.platform.GlStateManager._deleteTexture(resolvedPlane);
        com.mojang.blaze3d.platform.GlStateManager._deleteTexture(resolvedMotion);com.mojang.blaze3d.platform.GlStateManager._deleteTexture(resolvedDepth);
        if(resolvedFbo!=0)GL30.glDeleteFramebuffers(resolvedFbo);resolvedFbo=resolvedMotion=resolvedDepth=resolvedPlane=0;resolved=false;
    }
    @Override public void close() {
        deleteTextures();normalize.close();resolveDepth.close();mergeFlywheel.close();geometry.close();clouds.close();sodiumClouds.close();GL30.glDeleteFramebuffers(fbo);GL30.glDeleteVertexArrays(vao);
        GL15.glDeleteBuffers(vbo);MemoryUtil.memFree(vertices);
    }
}
