package org.kpc.rescaleframecraft.render;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

class NativeBridgeTest {
    @Test void stagedJniPreservesCallerBuffersWhenStatusIsUnavailable() {
        var library=Path.of("build/native-resources/native/windows-x64/ReScaleFrame.Minecraft.dll").toAbsolutePath();
        assumeTrue(System.getProperty("os.name").startsWith("Windows")&&Files.isRegularFile(library));
        System.load(library.toString());
        var status=new long[16];Arrays.fill(status,-1);
        assertEquals(0,NativeBridge.statusCount(0,status));
        for(long field:status)assertEquals(-1,field);
        assertNull(NativeBridge.configure(0,1920,1080,1,1,13,0,1,1,0,false,new org.kpc.rescaleframecraft.client.TaaSettings().sharpening()),
            "Preset-bearing JNI signature rejects an absent native session");
    }
}
