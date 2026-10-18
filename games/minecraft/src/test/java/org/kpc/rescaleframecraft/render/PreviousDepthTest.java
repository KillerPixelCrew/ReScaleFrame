package org.kpc.rescaleframecraft.render;

import org.joml.Matrix4f;
import org.joml.Vector4f;
import org.junit.jupiter.api.Test;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import static org.junit.jupiter.api.Assertions.*;

class PreviousDepthTest {
    @Test void priorClipDecoderMatchesPriorWorldProjectionAcrossDepthConventions() {
        for(boolean reversed:new boolean[]{false,true})for(boolean zeroToOne:new boolean[]{false,true}) {
            var canonical=new Matrix4f().perspective(1,1,.05f,1024);
            var raw=TemporalCamera.canonicalDepth(reversed,zeroToOne).invert().mul(canonical);
            var camera=new TemporalCamera();Object world=new Object();
            camera.begin(new Matrix4f(),raw,0,0,0,64,64,world,reversed,zeroToOne);
            // Changing the far plane must still decode the captured previous frame.
            var current=new Matrix4f(raw).m32(raw.m32()*1.2f);
            camera.begin(new Matrix4f(),current,0,0,0,64,64,world,reversed,zeroToOne);
            for(float distance:new float[]{.1f,2,64,512}) {
                var clip=raw.transform(new Vector4f(0,0,-distance,1));
                float inverse=camera.previousDepthDecode.x*clip.z/clip.w+camera.previousDepthDecode.y;
                assertEquals(1/distance,inverse,.00001);
            }
            camera.invalidate();camera.begin(new Matrix4f(),current,0,0,0,64,64,world,reversed,zeroToOne);
            var clip=current.transform(new Vector4f(0,0,-2,1));
            assertEquals(.5,camera.previousDepthDecode.x*clip.z/clip.w+camera.previousDepthDecode.y,.00001);
        }
    }
}
