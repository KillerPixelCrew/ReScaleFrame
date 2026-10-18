package org.kpc.rescaleframecraft.client;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;
import static org.kpc.rescaleframecraft.client.TaaSettings.FrameGeneration.*;

class FrameGenerationLimitsTest {
    @Test void limitsFollowEachProvidersReportedCapacityAndAutomaticSelection() {
        var limits=new FrameGenerationLimits();
        assertEquals(1,limits.maximum(DLSS),"Unknown hardware must not advertise arbitrary MFG multipliers");
        limits.record(DLSS.id,DLSS.id,3,true);
        limits.record(FSR_3.id,FSR_3.id,1,true);
        limits.record(XESS.id,XESS.id,3,true);
        assertEquals(3,limits.maximum(DLSS));assertEquals(1,limits.maximum(FSR_3));assertEquals(3,limits.maximum(XESS));
        assertEquals(3,limits.clamp(DLSS,10));assertEquals(1,limits.clamp(FSR_3,10));
        limits.record(AUTO.id,DLSS.id,5,true);assertEquals(5,limits.maximum(AUTO));
        limits.record(AUTO.id,FSR_3.id,1,true);assertEquals(1,limits.maximum(AUTO));
        assertEquals(5,limits.maximum(DLSS),"Another provider must not overwrite DLSS capacity");
        limits.record(DLSS.id,DLSS.id,0,false);assertEquals(5,limits.maximum(DLSS),"A transient pending switch has no fresh capability report");
        limits.reset();assertEquals(1,limits.maximum(XESS));assertEquals(1,limits.maximum(AUTO));
    }
    @Test void anOpenRangeTracksNativeCapabilityChangesAndClampsToTheNearestEndpoint() {
        var limits=new FrameGenerationLimits();var range=limits.range(DLSS);
        assertEquals(1,range.max());limits.record(DLSS.id,DLSS.id,5,true);assertEquals(5,range.max());
        assertEquals(5,range.getValidatedValue(10,()->1));
        limits.record(DLSS.id,DLSS.id,3,true);assertEquals(3,range.max());
        assertEquals(3,range.getValidatedValue(5,()->1));assertEquals(1,range.getValidatedValue(-1,()->1));
    }
}
