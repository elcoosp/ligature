<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);
const tag = ref("in");
</script>
<template>
  <div>
    <b v-if="on" class="on">{{ tag }}</b>
    <i v-else class="off">{{ tag }}</i>
    <span v-if="on">live</span>
    <Badge v-else note="off" />
  </div>
</template>
<script setup lang="ts">
defineOptions({ name: "Badge" });
const { note } = defineProps<{ note: string }>();
</script>
<template>
  <em class="badge">{{ note }}</em>
</template>
