package org.kpc.rescaleframecraft.render;

import org.joml.Matrix4f;
import org.joml.Vector4f;
import org.junit.jupiter.api.Test;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import static org.junit.jupiter.api.Assertions.*;

class DistantHorizonsProjectionTest {
    @Test void extendedDepthCoversDistantTerrainAndPreservesOffAxisProjection() {
        for(boolean reversed:new boolean[]{false,true})for(boolean zeroToOne:new boolean[]{false,true}) {
            var canonical=new Matrix4f().perspective(1.1f,1.7f,.05f,1024).m20(.013f).m21(-.007f);
            var remap=TemporalCamera.canonicalDepth(reversed,zeroToOne);
            var raw=new Matrix4f(remap).invert().mul(canonical);
            DistantHorizonsIntegration.extendProjection(raw,16384,reversed,zeroToOne,new Matrix4f(),new Matrix4f());
            var result=new Matrix4f(remap).mul(raw);
            assertEquals(canonical.m00(),result.m00());assertEquals(canonical.m11(),result.m11());
            assertEquals(canonical.m20(),result.m20());assertEquals(canonical.m21(),result.m21());
            assertEquals(.05,result.perspectiveNear(),.000001);
            var clip=result.transform(new Vector4f(0,0,-8192,1));
            assertTrue(clip.z/clip.w<1,"Distant geometry must remain inside the shared depth domain");
        }
    }
}
