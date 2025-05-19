export function LoadingSpinner({ size = 'medium' }: { size?: 'small' | 'medium' | 'large' }) {
  const sizeClasses = {
    small: 'w-5 h-5',
    medium: 'w-8 h-8',
    large: 'w-12 h-12',
  };

  return (
    <div className="flex justify-center">
      <div
        className={`animate-spin rounded-full border-t-2 border-b-2 border-zinc-500 ${sizeClasses[size]}`}
      ></div>
    </div>
  );
}

export function FullPageLoader({ message }: { message?: string }) {
  return (
    <div className="flex flex-col items-center justify-center">
      <LoadingSpinner size="large" />
      {message && (
        <p className="mt-4 text-sm text-zinc-500">{message}</p>
      )}
    </div>
  );
}