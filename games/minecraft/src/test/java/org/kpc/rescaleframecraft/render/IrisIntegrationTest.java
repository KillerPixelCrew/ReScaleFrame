package org.kpc.rescaleframecraft.render;

import java.util.Map;
import java.util.List;
import net.irisshaders.iris.shaderpack.transform.line.LineTransform;
import net.irisshaders.iris.shaderpack.include.AbsolutePackPath;
import net.irisshaders.iris.shaderpack.option.OptionAnnotatedSource;
import net.irisshaders.iris.shaderpack.option.values.MutableOptionValues;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class IrisIntegrationTest {
    @Test void externalTemporalOwnerKeepsAtmosphericSamplingButDoesNotEnablePackJitterOrResolve() {
        var lines=List.of("#ifdef TAA","    dither = fract(dither + goldenRatio * mod(float(frameCounter), 3600.0));","#endif",
            "#ifdef TAA","    return fract(n + goldenRatio * mod(float(frameCounter), 3600.0));","#else","    return fract(n);","#endif",
            "#ifdef TAA","    position.xy = TAAJitter(position.xy,position.w);","#endif",
            "#ifdef TAA","    DoTAA(color,temp,z1);","#endif");
        LineTransform original=(index,line)->line;
        var transform=IrisIntegration.runtimeSampling(original,lines,true);
        assertTrue(transform.transform(0,lines.get(0)).startsWith("#if 1"));
        assertTrue(transform.transform(3,lines.get(3)).startsWith("#if 1"));
        assertEquals("#ifdef TAA",transform.transform(8,lines.get(8)));
        assertEquals("#ifdef TAA",transform.transform(11,lines.get(11)));
        assertSame(original,IrisIntegration.runtimeSampling(original,lines,false));
        assertEquals("#ifdef TAA",lines.get(0),"Stored shader source remains unchanged");
    }
    @Test void temporalOwnershipChangesEffectiveOptionsWithoutChangingPackPreferences() {
        var source=new OptionAnnotatedSource("""
            #define TAA_DEFINE -1 //[-1 0 1]
            #define TAA_ENABLED
            #ifdef TAA_ENABLED
            #endif
            #define TAA 2 //[0 1 2]
            """);
        var options=source.getOptionSet(AbsolutePackPath.fromAbsolutePath("/test.glsl"),source.getBooleanDefineReferences().keySet());
        var preferences=new MutableOptionValues(options,Map.of("TAA_DEFINE","1"));
            var effective=IrisIntegration.runtimeOptions(preferences,true);
            assertEquals("0",effective.getStringValueOrDefault("TAA_DEFINE"));
            assertFalse(effective.getBooleanValueOrDefault("TAA_ENABLED"));
            assertEquals("2",effective.getStringValueOrDefault("TAA"),"A numeric option must never receive the boolean false string");
            assertEquals("1",preferences.getStringValueOrDefault("TAA_DEFINE"));
            assertTrue(preferences.getBooleanValueOrDefault("TAA_ENABLED"));
            assertSame(preferences,IrisIntegration.runtimeOptions(preferences,false));
            assertEquals("0",IrisIntegration.runtimeOptions(preferences,true).getStringValueOrDefault("TAA_DEFINE"));
    }
}
