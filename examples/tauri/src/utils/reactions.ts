// 表情反应配置
export const REACTION_EMOJIS = [
  { emoji: '👍', name: '赞', color: '#07C160' },
  { emoji: '❤️', name: '喜欢', color: '#FF4D4F' },
  { emoji: '😂', name: '笑哭', color: '#FFA940' },
  { emoji: '😮', name: '惊讶', color: '#1890FF' },
  { emoji: '😢', name: '悲伤', color: '#722ED1' },
  { emoji: '😡', name: '愤怒', color: '#FA541C' },
  { emoji: '👏', name: '鼓掌', color: '#52C41A' },
  { emoji: '🎉', name: '庆祝', color: '#FAAD14' }
];

// 反应状态管理
export interface ReactionState {
  emoji: string;
  count: number;
  isActive: boolean;
  userIds: string[];
}

/**
 * 解析消息反应数据
 */
export function parseReactions(reactions: any[], currentUserId: string): ReactionState[] {
  if (!reactions || !Array.isArray(reactions)) {
    return [];
  }

  return reactions.map(reaction => {
    const userIdsRaw = reaction.userIds ?? reaction.user_ids ?? [];
    const userIds = Array.isArray(userIdsRaw) ? userIdsRaw.map((id: unknown) => String(id)) : [];
    const count = Number(reaction.count ?? userIds.length ?? 0);
    return {
      emoji: String(reaction.emoji ?? ''),
      count: Number.isFinite(count) ? count : userIds.length,
      isActive: !!currentUserId && userIds.includes(currentUserId),
      userIds,
    };
  });
}

/**
 * 获取反应颜色
 */
export function getReactionColor(emoji: string): string {
  const reaction = REACTION_EMOJIS.find(r => r.emoji === emoji);
  return reaction?.color || '#8C8C8C';
}

/**
 * 获取反应名称
 */
export function getReactionName(emoji: string): string {
  const reaction = REACTION_EMOJIS.find(r => r.emoji === emoji);
  return reaction?.name || emoji;
}

/**
 * 格式化反应用户列表
 */
export function formatReactionUsers(userIds: string[], currentUserId: string): string {
  const total = userIds.length;
  if (total === 0) return '';
  
  const includesCurrent = userIds.includes(currentUserId);
  const otherUsers = userIds.filter(id => id !== currentUserId);
  
  if (total === 1) {
    return includesCurrent ? '你' : '某人';
  }
  
  if (includesCurrent) {
    if (otherUsers.length === 1) {
      return '你和其他 1 人';
    } else {
      return `你和其他 ${otherUsers.length} 人`;
    }
  } else {
    return `${total} 人`;
  }
}

/**
 * 创建反应工具提示
 */
export function createReactionTooltip(reaction: ReactionState, currentUserId: string): string {
  const users = formatReactionUsers(reaction.userIds, currentUserId);
  const name = getReactionName(reaction.emoji);
  
  if (users) {
    return `${users} 添加了 ${name}`;
  }
  return name;
}

/**
 * 反应动画配置
 */
export const REACTION_ANIMATIONS = {
  scaleIn: {
    initial: { scale: 0, opacity: 0 },
    animate: { scale: 1, opacity: 1 },
    exit: { scale: 0, opacity: 0 },
    transition: { duration: 0.2, ease: 'easeOut' }
  },
  bounce: {
    initial: { scale: 0.8 },
    animate: { 
      scale: [0.8, 1.2, 1],
      transition: { duration: 0.3, ease: 'easeOut' }
    }
  },
  pulse: {
    animate: {
      scale: [1, 1.05, 1],
      transition: { duration: 0.6, repeat: Infinity }
    }
  }
};

/**
 * 反应样式配置
 */
export const REACTION_STYLES = {
  container: {
    base: 'inline-flex items-center gap-1 px-2 py-1 rounded-full text-sm transition-all duration-200',
    active: 'bg-green-100 text-green-700 border border-green-200',
    inactive: 'bg-gray-100 text-gray-600 border border-gray-200 hover:bg-gray-200'
  },
  emoji: {
    base: 'text-base leading-none',
    active: 'animate-bounce',
    inactive: ''
  },
  count: {
    base: 'text-xs font-medium',
    active: 'text-green-600',
    inactive: 'text-gray-500'
  }
};

export default {
  REACTION_EMOJIS,
  parseReactions,
  getReactionColor,
  getReactionName,
  formatReactionUsers,
  createReactionTooltip,
  REACTION_ANIMATIONS,
  REACTION_STYLES
};
