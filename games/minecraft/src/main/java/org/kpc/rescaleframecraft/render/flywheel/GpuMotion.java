package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.IntBuffer;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.Map;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryStack;

/** Retains final GPU-deformed vertices and replays real GPU draws into separate motion targets. */
public final class GpuMotion {
    private static final Map<Object,Group> groups=new IdentityHashMap<>();
    private static final Map<Integer,MotionProgram> programs=new HashMap<>();
    private static TemporalCamera camera;
    private static Group current;
    private static long serial;
    private static boolean active,immediate,actor;
    private static GpuHistoryBuffer validity;
    private static int fbo,motion,depth,plane,actorIds,testDepth,width,height,depthFormat,draws,meshOffset,meshOverride=-1;
    private GpuMotion() {}

    static final class Group implements AutoCloseable {
        final InstanceIndexHistory identities=new InstanceIndexHistory();
        final GpuHistoryBuffer indexTable=new GpuHistoryBuffer(GL30.GL_RG32UI),meshes=new GpuHistoryBuffer(GL30.GL_R32UI);
        GpuHistoryBuffer previous=new GpuHistoryBuffer(GL30.GL_RGBA32UI),current=new GpuHistoryBuffer(GL30.GL_RGBA32UI);
        long frame=-1;
        int vertexCount;
        IntBuffer meshData=BufferUtils.createIntBuffer(1);
        boolean begin(int slots) {
            if(frame==serial)return false;
            if(frame!=serial-1)identities.clear();
            frame=serial;vertexCount=0;
            var swap=previous;previous=current;current=swap;
            identities.begin(serial,slots);return true;
        }
        void record(Object instance,Object model,int gpuSlot,int count) {
            identities.record(instance,model,gpuSlot,vertexCount,count);
            vertexCount=Math.addExact(vertexCount,count);
        }
        void ready() {
            identities.end();indexTable.upload(identities.data());
            current.allocate((long)vertexCount*32);previous.allocate(32);
        }
        @Override public void close() {indexTable.close();meshes.close();previous.close();current.close();identities.clear();}
    }
    public static void begin(TemporalCamera view,int sourceDepth) {
        camera=view;serial++;if((int)serial==0) {release();serial++;}
        active=GL.getCapabilities().OpenGL43;draws=0;current=null;immediate=false;actor=false;
        if(!active)return;
        if(camera.reset)for(var group:groups.values()) {group.identities.clear();group.frame=-1;}
        int format=sourceDepthFormat(sourceDepth);
        if(width!=view.width()||height!=view.height()||depthFormat!=format||fbo==0)createTargets(format);
        int oldDraw=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING);
        boolean scissor=GL11.glIsEnabled(GL11.GL_SCISSOR_TEST),write=GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
        try(var stack=MemoryStack.stackPush()) {
            var mask=stack.malloc(16);
            for(int i=0;i<4;i++) {mask.position(i*4);GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,i,mask);}
            GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);GL11.glDisable(GL11.GL_SCISSOR_TEST);
            for(int i=0;i<4;i++)GL30.glColorMaski(i,true,true,true,true);GL11.glDepthMask(true);
            GL30.glClearBufferfv(GL11.GL_COLOR,0,new float[]{0,0,1,-1});
            GL30.glClearBufferfv(GL11.GL_COLOR,1,new float[]{1,0,0,0});
            GL30.glClearBufferfv(GL11.GL_COLOR,2,new float[]{0,0,0,0});
            GL30.glClearBufferuiv(GL11.GL_COLOR,3,new int[]{0,0,0,0});
            GL30.glClearBufferfv(GL11.GL_DEPTH,0,new float[]{view.reversedDepth?0:1});
            for(int i=0;i<4;i++)GL30.glColorMaski(i,mask.get(i*4)!=0,mask.get(i*4+1)!=0,mask.get(i*4+2)!=0,mask.get(i*4+3)!=0);
        } finally {
            GL11.glDepthMask(write);if(scissor)GL11.glEnable(GL11.GL_SCISSOR_TEST);
            GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,oldDraw);
        }
    }
    static Group group(Object owner) {return groups.computeIfAbsent(owner,key->new Group());}
    static boolean capturing() {return active&&!IrisState.shadowPass();}
    static void select(Group group,int offset,int override) {current=group;meshOffset=offset;meshOverride=override;}
    static Group selected() {return current;}
    static void overrideMesh(int offset) {meshOverride=offset;}
    static void immediate(Object owner,Object topology,int vertices,Runnable original) {
        if(!capturing()) {original.run();return;}
        var group=group(owner);
        if(group.begin(1)) {
            group.record(owner,topology,0,vertices);
            group.meshData.clear().put(0).flip();group.meshes.upload(group.meshData);group.ready();
        }
        scoped(group,original);
    }
    static void scoped(Group group,Runnable original) {
        scoped(group,false,original);
    }
    static void scoped(Group group,boolean actors,Runnable original) {
        Group saved=current;int savedOffset=meshOffset,savedOverride=meshOverride;boolean savedImmediate=immediate,savedActor=actor;
        select(group,0,-1);immediate=true;actor=actors;
        try {original.run();} finally {select(saved,savedOffset,savedOverride);immediate=savedImmediate;actor=savedActor;}
    }
    public static void drawImmediate(Runnable original) {if(ActorMotion.drawing())ActorMotion.draw(original);else if(immediate)draw(original);else original.run();}
    public static void draw(Runnable original) {
        original.run();
        if(!active||current==null||current.vertexCount==0||IrisState.shadowPass())return;
        int source=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM);
        var program=programs.get(source);
        if(program==null) {
            program=new MotionProgram(source);programs.put(source,program);
            ReScaleFrameCraft.LOGGER.info("GPU motion program linked: source={}, history binding={}",source,program.storageBinding);
        }
        int target=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING),activeTexture=GL11.glGetInteger(GL13.GL_ACTIVE_TEXTURE);
        int storage=GL30.glGetIntegeri(GL43.GL_SHADER_STORAGE_BUFFER_BINDING,program.storageBinding);
        int genericStorage=GL11.glGetInteger(GL43.GL_SHADER_STORAGE_BUFFER_BINDING);
        long storageStart=GL32.glGetInteger64i(GL43.GL_SHADER_STORAGE_BUFFER_START,program.storageBinding);
        long storageSize=GL32.glGetInteger64i(GL43.GL_SHADER_STORAGE_BUFFER_SIZE,program.storageBinding);
        int func=GL11.glGetInteger(GL11.GL_DEPTH_FUNC);boolean test=GL11.glIsEnabled(GL11.GL_DEPTH_TEST);
        boolean write=GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
        try(var stack=MemoryStack.stackPush()) {
            var mask0=stack.malloc(4);var mask1=stack.malloc(4);var mask2=stack.malloc(4);var mask3=stack.malloc(4);var oldTextures=stack.mallocInt(3);
            GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,0,mask0);GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,1,mask1);
            GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,2,mask2);
            GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,3,mask3);
            boolean blend0=GL30.glIsEnabledi(GL11.GL_BLEND,0),blend1=GL30.glIsEnabledi(GL11.GL_BLEND,1);
            boolean blend2=GL30.glIsEnabledi(GL11.GL_BLEND,2);
            boolean blend3=GL30.glIsEnabledi(GL11.GL_BLEND,3);
            int bound=0;
            try {
                program.bind();
                for(int i=0;i<3;i++) {
                    GL13.glActiveTexture(GL13.GL_TEXTURE0+program.textureUnits[i]);oldTextures.put(i,GL11.glGetInteger(GL31.GL_TEXTURE_BINDING_BUFFER));
                    GL11.glBindTexture(GL31.GL_TEXTURE_BUFFER,switch(i) {case 0->current.previous.texture();case 1->current.indexTable.texture();default->current.meshes.texture();});bound++;
                }
                GL30.glBindBufferBase(GL43.GL_SHADER_STORAGE_BUFFER,program.storageBinding,current.current.buffer());
                program.unsigned("_rfc_frame",(int)serial);program.unsigned("_rfc_previousFrame",(int)(serial-1));
                program.unsigned("_rfc_instancedMeshOffset",meshOffset);program.integer("_rfc_meshOverride",meshOverride);
                program.vector("_rfc_renderSize",width,height);program.vector("_rfc_currentJitter",camera.jitter.x,camera.jitter.y);
                program.integer("_rfc_reversedDepth",camera.reversedDepth?1:0);
                program.integer("_rfc_actorVertices",actor?1:0);
                program.integer("_rfc_useDepthDecode",program.immediate&&!program.terrain&&!actor?1:0);
                program.vector("_rfc_depthDecode",camera.previousDepthDecode.x,camera.previousDepthDecode.y);
                GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);
                GL11.glEnable(GL11.GL_DEPTH_TEST);GL11.glDepthFunc(camera.reversedDepth?GL11.GL_GEQUAL:GL11.GL_LEQUAL);
                GL11.glDepthMask(true);GL30.glDisablei(GL11.GL_BLEND,0);GL30.glDisablei(GL11.GL_BLEND,1);
                GL30.glDisablei(GL11.GL_BLEND,2);GL30.glColorMaski(2,true,true,true,true);
                GL30.glDisablei(GL11.GL_BLEND,3);GL30.glColorMaski(3,true,true,true,true);
                GL30.glColorMaski(0,true,true,true,true);GL30.glColorMaski(1,true,true,true,true);
                original.run();draws++;
            } finally {
                GL30.glColorMaski(0,mask0.get(0)!=0,mask0.get(1)!=0,mask0.get(2)!=0,mask0.get(3)!=0);
                GL30.glColorMaski(1,mask1.get(0)!=0,mask1.get(1)!=0,mask1.get(2)!=0,mask1.get(3)!=0);
                GL30.glColorMaski(2,mask2.get(0)!=0,mask2.get(1)!=0,mask2.get(2)!=0,mask2.get(3)!=0);
                GL30.glColorMaski(3,mask3.get(0)!=0,mask3.get(1)!=0,mask3.get(2)!=0,mask3.get(3)!=0);
                if(blend0)GL30.glEnablei(GL11.GL_BLEND,0);if(blend1)GL30.glEnablei(GL11.GL_BLEND,1);
                if(blend2)GL30.glEnablei(GL11.GL_BLEND,2);
                if(blend3)GL30.glEnablei(GL11.GL_BLEND,3);
                for(int i=0;i<bound;i++) {GL13.glActiveTexture(GL13.GL_TEXTURE0+program.textureUnits[i]);GL11.glBindTexture(GL31.GL_TEXTURE_BUFFER,oldTextures.get(i));}
                GL13.glActiveTexture(activeTexture);
                if(storage!=0&&storageSize>0)GL30.glBindBufferRange(GL43.GL_SHADER_STORAGE_BUFFER,program.storageBinding,storage,storageStart,storageSize);
                else GL30.glBindBufferBase(GL43.GL_SHADER_STORAGE_BUFFER,program.storageBinding,storage);
                GL15.glBindBuffer(GL43.GL_SHADER_STORAGE_BUFFER,genericStorage);
                GL20.glUseProgram(source);GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,target);
                GL11.glDepthFunc(func);GL11.glDepthMask(write);
                if(!test)GL11.glDisable(GL11.GL_DEPTH_TEST);
            }
        }
    }
    public static void end() {
        if(!active)return;
        GL42.glMemoryBarrier(GL43.GL_SHADER_STORAGE_BARRIER_BIT|GL42.GL_TEXTURE_FETCH_BARRIER_BIT);
        active=false;current=null;
        groups.entrySet().removeIf(entry->{if(entry.getValue().frame==serial)return false;entry.getValue().close();return true;});
    }
    public static int motion() {return motion;}
    public static int depth() {return depth;}
    public static int plane() {return plane;}
    public static int actorIds() {return actorIds;}
    public static int actorValidity() {return validity==null?0:validity.texture();}
    static void actorValidity(IntBuffer flags) {if(validity==null)validity=new GpuHistoryBuffer(GL30.GL_R32UI);validity.upload(flags);}
    public static boolean present() {return draws>0;}
    public static void forget(int source) {var program=programs.remove(source);if(program!=null)program.close();ActorMotion.forget(source);}
    public static void release() {
        active=false;current=null;draws=0;
        groups.values().forEach(Group::close);groups.clear();programs.values().forEach(MotionProgram::close);programs.clear();deleteTargets();
        if(validity!=null) {validity.close();validity=null;}ActorMotion.clear();
    }
    private static int sourceDepthFormat(int sourceDepth) {
        int texture=GL11.glGetInteger(GL11.GL_TEXTURE_BINDING_2D);
        GL11.glBindTexture(GL11.GL_TEXTURE_2D,sourceDepth);int format=GL11.glGetTexLevelParameteri(GL11.GL_TEXTURE_2D,0,GL11.GL_TEXTURE_INTERNAL_FORMAT);
        GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
        if(format==GL30.GL_DEPTH24_STENCIL8)format=GL14.GL_DEPTH_COMPONENT24;
        if(format==GL30.GL_DEPTH32F_STENCIL8)format=GL30.GL_DEPTH_COMPONENT32F;
        return format;
    }
    private static void createTargets(int format) {
        int old=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING),texture=GL11.glGetInteger(GL11.GL_TEXTURE_BINDING_2D);
        deleteTargets();width=camera.width();height=camera.height();depthFormat=format;
        fbo=GL30.glGenFramebuffers();GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);
        motion=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);depth=texture(GL30.GL_R32F,GL11.GL_RED);plane=texture(GL30.GL_RG16F,GL30.GL_RG);testDepth=texture(format,GL11.GL_DEPTH_COMPONENT);
        actorIds=texture(GL30.GL_R32UI,GL30.GL_RED_INTEGER);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,motion,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,depth,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT2,GL11.GL_TEXTURE_2D,plane,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT3,GL11.GL_TEXTURE_2D,actorIds,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_DEPTH_ATTACHMENT,GL11.GL_TEXTURE_2D,testDepth,0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2,GL30.GL_COLOR_ATTACHMENT3});
        if(GL30.glCheckFramebufferStatus(GL30.GL_DRAW_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Incomplete GPU motion framebuffer");
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,old);GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
    }
    private static int texture(int format,int external) {
        int texture=GL11.glGenTextures();GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
        GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,width,height,0,external,format==GL30.GL_R32UI?GL11.GL_UNSIGNED_INT:GL11.GL_FLOAT,0L);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MAG_FILTER,GL11.GL_NEAREST);return texture;
    }
    private static void deleteTargets() {
        GL11.glDeleteTextures(motion);GL11.glDeleteTextures(depth);GL11.glDeleteTextures(plane);GL11.glDeleteTextures(actorIds);GL11.glDeleteTextures(testDepth);if(fbo!=0)GL30.glDeleteFramebuffers(fbo);
        motion=depth=plane=actorIds=testDepth=fbo=width=height=0;
    }
}
