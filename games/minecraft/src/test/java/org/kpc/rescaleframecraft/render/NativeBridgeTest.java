package org.kpc.rescaleframecraft.render;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

class NativeBridgeTest {
    @Test void stagedJniWritesStatusIntoTheCallerBuffer() {
        var library=Path.of("build/native-resources/native/windows-x64/ReScaleFrame.Minecraft.dll").toAbsolutePath();
        assumeTrue(System.getProperty("os.name").startsWith("Windows")&&Files.isRegularFile(library));
        System.load(library.toString());
        var status=new long[16];Arrays.fill(status,-1);
        NativeBridge.status(0,status);
        assertArrayEquals(new long[16],status,"JNI signature and capability output layout match the staged DLL");
        var legacy=new long[14];Arrays.fill(legacy,-1);NativeBridge.status(0,legacy);
        assertArrayEquals(new long[14],legacy,"The appended capability fields preserve legacy callers");
        assertNull(NativeBridge.configure(0,1920,1080,1,1,13,0,1,1,0,false),
            "Preset-bearing JNI signature rejects an absent native session");
    }
}
