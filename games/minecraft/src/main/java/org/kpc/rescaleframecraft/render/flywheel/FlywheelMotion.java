package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.IntBuffer;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.Map;
import dev.engine_room.flywheel.api.model.Model;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.mixin.flywheel.*;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryStack;

/** Retains final GPU-deformed vertices and replays real Flywheel draws into separate motion targets. */
public final class FlywheelMotion {
    private static final Map<Object,Group> groups=new IdentityHashMap<>();
    private static final Map<Integer,MotionProgram> programs=new HashMap<>();
    private static TemporalCamera camera;
    private static Group current;
    private static long serial;
    private static boolean active;
    private static int fbo,motion,depth,testDepth,width,height,depthFormat,draws,meshOffset,meshOverride=-1;
    private FlywheelMotion() {}

    private static final class Group implements AutoCloseable {
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
        void record(Object instance,Model model,int gpuSlot) {
            int count=vertices(model);
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
        active=GL.getCapabilities().OpenGL43;draws=0;current=null;
        if(!active)return;
        if(camera.reset)for(var group:groups.values()) {group.identities.clear();group.frame=-1;}
        int format=sourceDepthFormat(sourceDepth);
        if(width!=view.width()||height!=view.height()||depthFormat!=format||fbo==0)createTargets(format);
        int oldDraw=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING);
        boolean scissor=GL11.glIsEnabled(GL11.GL_SCISSOR_TEST),write=GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
        try(var stack=MemoryStack.stackPush()) {
            var mask=stack.malloc(4);GL11.glGetBooleanv(GL11.GL_COLOR_WRITEMASK,mask);
            GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);GL11.glDisable(GL11.GL_SCISSOR_TEST);
            GL11.glColorMask(true,true,true,true);GL11.glDepthMask(true);
            GL30.glClearBufferfv(GL11.GL_COLOR,0,new float[]{0,0,1,-1});
            GL30.glClearBufferfv(GL11.GL_COLOR,1,new float[]{1,0,0,0});
            GL30.glClearBufferfv(GL11.GL_DEPTH,0,new float[]{view.reversedDepth?0:1});
            GL11.glColorMask(mask.get(0)!=0,mask.get(1)!=0,mask.get(2)!=0,mask.get(3)!=0);
        } finally {
            GL11.glDepthMask(write);if(scissor)GL11.glEnable(GL11.GL_SCISSOR_TEST);
            GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,oldDraw);
        }
    }
    public static void indirect(Object owner) {
        current=null;meshOverride=-1;if(!active||IrisState.shadowPass())return;
        var access=(IndirectGroupAccessor)owner;
        var group=groups.computeIfAbsent(owner,key->new Group());
        if(group.begin(Math.toIntExact(access.rfc$buffers().objectStorage.objectBuffer.capacity()/access.rfc$stride()))) {
            for(var instancer:access.rfc$instancers()) {
                Object[] pages=(Object[])((IndirectInstancerAccessor)instancer).rfc$pages().get();
                Model model=instancer.recreate.key().model();
                for(int page=0;page<pages.length;page++) {
                    var contents=(InstancePageAccessor)pages[page];int valid=contents.rfc$valid().get();
                    while(valid!=0) {
                        int slot=Integer.numberOfTrailingZeros(valid);valid&=valid-1;
                        group.record(contents.rfc$instances()[slot],model,instancer.local2GlobalInstanceIndex(page*32+slot));
                    }
                }
            }
            var list=access.rfc$draws();
            if(group.meshData.capacity()<list.size())group.meshData=BufferUtils.createIntBuffer(list.size());
            group.meshData.clear().limit(Math.max(1,list.size()));
            for(int i=0;i<list.size();i++) {
                var draw=list.get(i);var instancer=((IndirectDrawAccessor)draw).rfc$instancer();
                group.meshData.put(i,prefix(instancer.recreate.key().model(),draw.indexOfMeshInModel()));
            }
            group.meshes.upload(group.meshData);group.ready();
        }
        current=group;
    }
    public static void instanced(Object owner) {
        current=null;meshOverride=-1;if(!active||IrisState.shadowPass())return;
        var draw=(InstancedDrawAccessor)owner;var instancer=draw.rfc$instancer();
        var group=groups.computeIfAbsent(instancer,key->new Group());
        var instances=((BaseInstancerAccessor)instancer).rfc$instances();Model model=instancer.recreate.key().model();
        if(group.begin(instances.size())) {
            for(int i=0;i<instances.size();i++)group.record(instances.get(i),model,i);
            group.meshData.clear().limit(1);group.meshData.put(0,0);group.meshes.upload(group.meshData);group.ready();
        }
        meshOffset=prefix(model,draw.rfc$meshIndex());current=group;
    }
    public static void colorwheelIndirect(Object owner) {
        current=null;meshOverride=-1;if(!active||IrisState.shadowPass())return;
        var access=(ColorwheelGroupAccessor)owner;
        var group=groups.computeIfAbsent(owner,key->new Group());
        if(group.begin(Math.toIntExact(access.rfc$buffers().objectStorage.objectBuffer.capacity()/access.rfc$stride()))) {
            for(var instancer:access.rfc$instancers()) {
                Object[] pages=(Object[])((IndirectInstancerAccessor)instancer).rfc$pages().get();
                Model model=instancer.recreate.key().model();
                for(int page=0;page<pages.length;page++) {
                    var contents=(InstancePageAccessor)pages[page];int valid=contents.rfc$valid().get();
                    while(valid!=0) {
                        int slot=Integer.numberOfTrailingZeros(valid);valid&=valid-1;
                        group.record(contents.rfc$instances()[slot],model,instancer.local2GlobalInstanceIndex(page*32+slot));
                    }
                }
            }
            var list=access.rfc$draws();
            if(group.meshData.capacity()<list.size())group.meshData=BufferUtils.createIntBuffer(list.size());
            group.meshData.clear().limit(Math.max(1,list.size()));
            for(int i=0;i<list.size();i++) {
                var draw=list.get(i);var instancer=((ColorwheelIndirectDrawAccessor)draw).rfc$instancer();
                group.meshData.put(i,prefix(instancer.recreate.key().model(),draw.indexOfMeshInModel()));
            }
            group.meshes.upload(group.meshData);group.ready();
        }
        current=group;
    }
    public static void colorwheelInstanced(Object owner) {
        current=null;meshOverride=-1;if(!active||IrisState.shadowPass())return;
        var draw=(ColorwheelInstancedDrawAccessor)owner;var instancer=draw.rfc$instancer();
        var group=groups.computeIfAbsent(instancer,key->new Group());
        var instances=((BaseInstancerAccessor)instancer).rfc$instances();Model model=instancer.recreate.key().model();
        if(group.begin(instances.size())) {
            for(int i=0;i<instances.size();i++)group.record(instances.get(i),model,i);
            group.meshData.clear().limit(1);group.meshData.put(0,0);group.meshes.upload(group.meshData);group.ready();
        }
        meshOffset=prefix(model,draw.rfc$meshIndex());current=group;
    }
    public static void crumbling(Object owner) {
        if(!active||current==null)return;
        if(owner instanceof ColorwheelIndirectDrawAccessor access) {
            var draw=(dev.djefrey.colorwheel.indirect.ClrwlIndirectDraw)owner;
            meshOverride=prefix(access.rfc$instancer().recreate.key().model(),draw.indexOfMeshInModel());
        } else {
            var draw=(dev.engine_room.flywheel.backend.engine.indirect.IndirectDraw)owner;
            meshOverride=prefix(((IndirectDrawAccessor)owner).rfc$instancer().recreate.key().model(),draw.indexOfMeshInModel());
        }
    }
    public static void draw(Runnable original) {
        original.run();
        if(!active||current==null||current.vertexCount==0||IrisState.shadowPass())return;
        int source=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM);
        var program=programs.get(source);
        if(program==null) {
            program=new MotionProgram(source);programs.put(source,program);
            ReScaleFrameCraft.LOGGER.info("Flywheel motion program linked: source={}, history binding={}",source,program.storageBinding);
        }
        int target=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING),activeTexture=GL11.glGetInteger(GL13.GL_ACTIVE_TEXTURE);
        int storage=GL30.glGetIntegeri(GL43.GL_SHADER_STORAGE_BUFFER_BINDING,program.storageBinding);
        int genericStorage=GL11.glGetInteger(GL43.GL_SHADER_STORAGE_BUFFER_BINDING);
        long storageStart=GL32.glGetInteger64i(GL43.GL_SHADER_STORAGE_BUFFER_START,program.storageBinding);
        long storageSize=GL32.glGetInteger64i(GL43.GL_SHADER_STORAGE_BUFFER_SIZE,program.storageBinding);
        int func=GL11.glGetInteger(GL11.GL_DEPTH_FUNC);boolean test=GL11.glIsEnabled(GL11.GL_DEPTH_TEST);
        boolean write=GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
        try(var stack=MemoryStack.stackPush()) {
            var mask0=stack.malloc(4);var mask1=stack.malloc(4);var oldTextures=stack.mallocInt(3);
            GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,0,mask0);GL30.glGetBooleani_v(GL11.GL_COLOR_WRITEMASK,1,mask1);
            boolean blend0=GL30.glIsEnabledi(GL11.GL_BLEND,0),blend1=GL30.glIsEnabledi(GL11.GL_BLEND,1);
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
                GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);
                GL11.glEnable(GL11.GL_DEPTH_TEST);GL11.glDepthFunc(camera.reversedDepth?GL11.GL_GEQUAL:GL11.GL_LEQUAL);
                GL11.glDepthMask(true);GL30.glDisablei(GL11.GL_BLEND,0);GL30.glDisablei(GL11.GL_BLEND,1);
                GL30.glColorMaski(0,true,true,true,true);GL30.glColorMaski(1,true,true,true,true);
                original.run();draws++;
            } finally {
                GL30.glColorMaski(0,mask0.get(0)!=0,mask0.get(1)!=0,mask0.get(2)!=0,mask0.get(3)!=0);
                GL30.glColorMaski(1,mask1.get(0)!=0,mask1.get(1)!=0,mask1.get(2)!=0,mask1.get(3)!=0);
                if(blend0)GL30.glEnablei(GL11.GL_BLEND,0);if(blend1)GL30.glEnablei(GL11.GL_BLEND,1);
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
        if(active)GL42.glMemoryBarrier(GL43.GL_SHADER_STORAGE_BARRIER_BIT|GL42.GL_TEXTURE_FETCH_BARRIER_BIT);
        active=false;current=null;
        groups.entrySet().removeIf(entry->{if(entry.getValue().frame==serial)return false;entry.getValue().close();return true;});
    }
    public static int motion() {return motion;}
    public static int depth() {return depth;}
    public static boolean present() {return draws>0;}
    public static void forget(int source) {var program=programs.remove(source);if(program!=null)program.close();}
    public static void release() {
        active=false;current=null;draws=0;
        groups.values().forEach(Group::close);groups.clear();programs.values().forEach(MotionProgram::close);programs.clear();deleteTargets();
    }
    private static int vertices(Model model) {int sum=0;for(var mesh:model.meshes())sum=Math.addExact(sum,mesh.mesh().vertexCount());return sum;}
    private static int prefix(Model model,int meshIndex) {int sum=0;for(int i=0;i<meshIndex;i++)sum+=model.meshes().get(i).mesh().vertexCount();return sum;}
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
        motion=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);depth=texture(GL30.GL_R32F,GL11.GL_RED);testDepth=texture(format,GL11.GL_DEPTH_COMPONENT);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,motion,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,depth,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_DEPTH_ATTACHMENT,GL11.GL_TEXTURE_2D,testDepth,0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1});
        if(GL30.glCheckFramebufferStatus(GL30.GL_DRAW_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Incomplete Flywheel motion framebuffer");
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,old);GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
    }
    private static int texture(int format,int external) {
        int texture=GL11.glGenTextures();GL11.glBindTexture(GL11.GL_TEXTURE_2D,texture);
        GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,width,height,0,external,GL11.GL_FLOAT,0L);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MAG_FILTER,GL11.GL_NEAREST);return texture;
    }
    private static void deleteTargets() {
        GL11.glDeleteTextures(motion);GL11.glDeleteTextures(depth);GL11.glDeleteTextures(testDepth);if(fbo!=0)GL30.glDeleteFramebuffers(fbo);
        motion=depth=testDepth=fbo=width=height=0;
    }
}
