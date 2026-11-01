; Transforms the position by the model view and projection matrices like GX and passes the color on

; $MODE = "UniformRegister"
; $SPI_VS_OUT_CONFIG.VS_EXPORT_COUNT = 0
; $NUM_SPI_VS_OUT_ID = 1
; $SPI_VS_OUT_ID[0].SEMANTIC_0 = 0

; C0 to C3, the rows of the projection matrix, like libogc's Mtx44
; $UNIFORM_VARS[0].name = "uProjection"
; $UNIFORM_VARS[0].type = "mat4"
; $UNIFORM_VARS[0].count = 1
; $UNIFORM_VARS[0].block = -1
; $UNIFORM_VARS[0].offset = 0
; C4 to C6, the rows of the model view matrix, like libogc's Mtx
; $UNIFORM_VARS[1].name = "uModelView"
; $UNIFORM_VARS[1].type = "mat3x4"
; $UNIFORM_VARS[1].count = 1
; $UNIFORM_VARS[1].block = -1
; $UNIFORM_VARS[1].offset = 16

; R1
; $ATTRIB_VARS[0].name = "aPosition"
; $ATTRIB_VARS[0].type = "vec4"
; $ATTRIB_VARS[0].location = 0
; R2
; $ATTRIB_VARS[1].name = "aColor"
; $ATTRIB_VARS[1].type = "vec4"
; $ATTRIB_VARS[1].location = 1

00 CALL_FS NO_BARRIER
01 ALU: ADDR(32) CNT(28)
    0  x: DOT4   R127.x, R1.x, C4.x
       y: DOT4   ____, R1.y, C4.y
       z: DOT4   ____, R1.z, C4.z
       w: DOT4   ____, R1.w, C4.w
    1  x: DOT4   ____, R1.x, C5.x
       y: DOT4   R127.y, R1.y, C5.y
       z: DOT4   ____, R1.z, C5.z
       w: DOT4   ____, R1.w, C5.w
    2  x: DOT4   ____, R1.x, C6.x
       y: DOT4   ____, R1.y, C6.y
       z: DOT4   R127.z, R1.z, C6.z
       w: DOT4   ____, R1.w, C6.w
    3  x: DOT4   R1.x, R127.x, C0.x
       y: DOT4   ____, R127.y, C0.y
       z: DOT4   ____, R127.z, C0.z
       w: DOT4   ____, R1.w, C0.w
    4  x: DOT4   ____, R127.x, C1.x
       y: DOT4   R1.y, R127.y, C1.y
       z: DOT4   ____, R127.z, C1.z
       w: DOT4   ____, R1.w, C1.w
    5  x: DOT4   ____, R127.x, C2.x
       y: DOT4   ____, R127.y, C2.y
       z: DOT4   R1.z, R127.z, C2.z
       w: DOT4   ____, R1.w, C2.w
    6  x: DOT4   ____, R127.x, C3.x
       y: DOT4   ____, R127.y, C3.y
       z: DOT4   ____, R127.z, C3.z
       w: DOT4   R1.w, R1.w, C3.w
02 EXP_DONE: POS0, R1
03 EXP_DONE: PARAM0, R2 NO_BARRIER
END_OF_PROGRAM
