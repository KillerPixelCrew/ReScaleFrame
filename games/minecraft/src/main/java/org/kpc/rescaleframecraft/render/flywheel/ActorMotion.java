package org.kpc.rescaleframecraft.render.flywheel;

import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexConsumer;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.BitSet;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.nio.IntBuffer;
import org.kpc.rescaleframecraft.temporal.GeometryHistory;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;
import org.lwjgl.opengl.GL20;

/** Carries object-local vertex identities through real buffered draws, independently of batch/sort order. */
public final class ActorMotion {
    private static final Object OWNER=new Object();
    private static final Map<GeometryHistory.Batch,Map<Integer,Part>> histories=new IdentityHashMap<>();
    private static final Map<MeshData,Stream> meshes=new ConcurrentHashMap<>();
    private static final ArrayList<Part> submitted=new ArrayList<>();
    private static final ThreadLocal<Emission> emission=ThreadLocal.withInitial(Emission::new);
    private static GpuMotion.Group group;
    private static Stream drawing;
    private static long frame;
    private static int next;
    private static volatile boolean active;
    private static IntBuffer mappings,batchIds,validity;
    private ActorMotion() {}
    private static final class Emission {GeometryHistory.Batch owner;int logical;}

    /** Parallel to BufferBuilder vertex memory. Quad sorting changes indices, not these slots. */
    public static final class Stream {
        final long capturedFrame=frame;
        GeometryHistory.Batch[] owners=new GeometryHistory.Batch[64];
        int[] ordinals=new int[64],logicals=new int[64];
        int size;
        void append(GeometryHistory.Batch owner,int logical) {
            if(size==owners.length) {
                owners=Arrays.copyOf(owners,size*2);ordinals=Arrays.copyOf(ordinals,size*2);logicals=Arrays.copyOf(logicals,size*2);
            }
            owners[size]=owner;logicals[size]=logical;ordinals[size]=owner==null?-1:owner.gpuVertices++;size++;
        }
        void duplicates(int count) {
            while(size<count)append(size==0?null:owners[size-1],size==0?-1:logicals[size-1]);
        }
    }
    private static final class Part {
        final GeometryHistory.Batch owner;
        int[] previous=new int[0],current=new int[0];
        BitSet oldUsed=new BitSet(),used=new BitSet();
        long seen=-1;
        int id,oldLogical,oldPhysical,logical,physical;
        boolean consecutive;
        Part(GeometryHistory.Batch owner) {this.owner=owner;}
        void begin() {
            if(seen==frame)return;
            consecutive=seen==frame-1;seen=frame;oldLogical=logical;oldPhysical=physical;
            var swap=previous;previous=current;current=swap;Arrays.fill(current,-1);
            var bits=oldUsed;oldUsed=used;used=bits;used.clear();
            id=submitted.size();submitted.add(this);
        }
        int address(int ordinal) {
            if(ordinal>=current.length) {
                int old=current.length;current=Arrays.copyOf(current,Math.max(ordinal+1,Math.max(64,old*2)));Arrays.fill(current,old,current.length,-1);
            }
            if(current[ordinal]<0)current[ordinal]=next++;
            used.set(ordinal);return current[ordinal];
        }
        int previous(int ordinal) {return consecutive&&ordinal<previous.length&&oldUsed.get(ordinal)?previous[ordinal]+1:0;}
        boolean finish() {
            logical=owner.vertices();physical=owner.gpuVertices;
            return consecutive&&oldLogical==logical&&oldPhysical==physical&&used.equals(oldUsed);
        }
    }
    public static void emit(GeometryHistory.Batch owner,int logical,VertexConsumer delegate,float x,float y,float z) {
        if(!active) {delegate.addVertex(x,y,z);return;}
        var context=emission.get();var old=context.owner;int oldLogical=context.logical;
        context.owner=owner;context.logical=logical;
        try {delegate.addVertex(x,y,z);} finally {context.owner=old;context.logical=oldLogical;}
    }
    public static Stream vertex(Stream stream,int vertices) {
        if(!active)return stream;
        if(stream!=null&&stream.capturedFrame!=frame)stream=null;
        var context=emission.get();
        if(stream==null&&context.owner==null)return null;
        if(stream==null)stream=new Stream();
        stream.duplicates(vertices-1);stream.append(context.owner,context.logical);return stream;
    }
    public static void mesh(Stream stream,MeshData mesh,int vertices) {
        if(!active||stream==null||stream.capturedFrame!=frame||mesh==null)return;
        stream.duplicates(vertices);meshes.put(mesh,stream);
    }
    public static void discard(MeshData mesh) {meshes.remove(mesh);}
    public static void scope(MeshData mesh,Runnable original) {
        var saved=drawing;drawing=active?meshes.remove(mesh):null;
        try {original.run();} finally {drawing=saved;}
    }
    static boolean drawing() {return active&&drawing!=null;}
    static void draw(Runnable original) {
        int program=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM);var mesh=drawing;
        mappings=storage(mappings,mesh.size*2);batchIds=storage(batchIds,mesh.size);
        var addresses=mappings;var ids=batchIds;
        GeometryHistory.Batch lastOwner=null;Part part=null;
        for(int i=0;i<mesh.size;i++) {
            var owner=mesh.owners[i];
            if(owner==null) {addresses.put(0).put(-1);ids.put(0);continue;}
            if(owner!=lastOwner) {part=histories.computeIfAbsent(owner,key->new HashMap<>()).computeIfAbsent(program,key->new Part(owner));part.begin();lastOwner=owner;}
            int ordinal=mesh.ordinals[i];addresses.put(part.previous(ordinal)).put(part.address(ordinal));ids.put(part.id);
            owner.gpuDrawn.set(mesh.logicals[i]);
        }
        addresses.flip();ids.flip();group.vertexCount=next;
        group.current.allocateRetaining((long)next*32);group.previous.allocate(32);
        group.indexTable.upload(addresses);group.meshes.upload(ids);
        GpuMotion.scoped(group,true,()->GpuMotion.draw(original));
    }
    public static void begin(boolean reset) {
        active=GpuMotion.capturing();drawing=null;meshes.clear();submitted.clear();submitted.add(null);next=0;frame++;
        if(reset)histories.clear();
        if(active) {group=GpuMotion.group(OWNER);group.begin(0);}
    }
    public static void end() {
        if(!active)return;
        validity=storage(validity,submitted.size());var valid=validity;valid.put(1);
        for(int i=1;i<submitted.size();i++) {
            var part=submitted.get(i);valid.put(part.finish()?1:0);
            part.owner.gpuCaptured=part.owner.gpuDrawn.cardinality()==part.owner.vertices();
        }
        valid.flip();GpuMotion.actorValidity(valid);active=false;drawing=null;meshes.clear();
        histories.values().removeIf(parts->{parts.values().removeIf(part->part.seen!=frame);return parts.isEmpty();});
    }
    static void forget(int program) {histories.values().removeIf(parts->{parts.remove(program);return parts.isEmpty();});}
    private static IntBuffer storage(IntBuffer data,int count) {
        if(data==null||data.capacity()<count)data=BufferUtils.createIntBuffer(Math.max(count,data==null?128:data.capacity()*2));
        data.clear().limit(count);return data;
    }
    static void clear() {active=false;drawing=null;group=null;meshes.clear();histories.clear();submitted.clear();mappings=batchIds=validity=null;}
}
