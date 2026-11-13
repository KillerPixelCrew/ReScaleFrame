package org.kpc.rescaleframecraft.render;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import static org.junit.jupiter.api.Assertions.*;

/** Verify the actual mapped engine methods targeted by the input mixins, without starting Minecraft. */
class InputBoundaryBytecodeTest {
    @Test void selectedGlfwPollFollowsPresentAndSimulationEndExistsWithoutRendering() throws IOException {
        var calls=new ArrayList<String>();
        read("com/mojang/blaze3d/systems/RenderSystem", "flipFrame",calls);
        assertEquals(List.of("pollEvents","glfwSwapBuffers","pollEvents"),calls);
        calls.clear();read("net/minecraft/client/Minecraft","runTick",calls);
        assertTrue(calls.indexOf("handleAccumulatedMovement")>=0);
        assertTrue(calls.indexOf("handleAccumulatedMovement")<calls.indexOf("fireRenderFramePre"));
    }
    private static void read(String owner,String target,List<String> calls) throws IOException {
        try(var input=InputBoundaryBytecodeTest.class.getClassLoader().getResourceAsStream(owner+".class")) {
            assertNotNull(input,"Mapped engine bytecode must be available for the boundary contract");
            new ClassReader(input).accept(new ClassVisitor(Opcodes.ASM9) {
                @Override public MethodVisitor visitMethod(int access,String name,String descriptor,String signature,String[] exceptions) {
                    if(!name.equals(target))return null;
                    return new MethodVisitor(Opcodes.ASM9) {
                        @Override public void visitMethodInsn(int opcode,String type,String method,String desc,boolean itf) {
                            if(method.equals("pollEvents")||method.equals("glfwSwapBuffers")||method.equals("handleAccumulatedMovement")||method.equals("fireRenderFramePre"))calls.add(method);
                        }
                    };
                }
            },ClassReader.SKIP_DEBUG|ClassReader.SKIP_FRAMES);
        }
    }
}
